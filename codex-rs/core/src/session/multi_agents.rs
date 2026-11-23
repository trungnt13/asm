use std::sync::Arc;

use crate::agent::types::ResolvedMultiAgentV2UsageHints;
use crate::config::MultiAgentV2Config;
use crate::context::ContextualUserFragment;
use crate::context::MultiAgentRoleInstructions;
use crate::context::MultiAgentUsageHint;
use crate::session::step_context::StepContext;
use codex_context_fragments::set_annotated_content;
use codex_context_fragments::to_annotated_content;
use codex_prompts::ResolvedMessage;
use codex_prompts::ResolvedModelMessages;
use codex_prompts::ResolvedMultiAgentMessages;
use codex_protocol::config_types::MultiAgentMode;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ContentItemProvenance;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;

const HOSTED_MULTI_AGENT_SKILL_GUIDANCE: &str = "Before calling any multi-agent tool, read the full multi-agent skill using its listed package locator. Skip the read only if the full skill text is already in your current context. If the read fails, make note of the blocker.";

pub(crate) fn remove_collaboration_role_instructions(input: &mut Vec<Arc<ResponseItem>>) {
    input.retain_mut(|item| {
        let ResponseItem::Message {
            role,
            content,
            internal_chat_message_metadata_passthrough,
            ..
        } = item.as_ref()
        else {
            return true;
        };
        if role != "developer" {
            return true;
        }
        let metadata = internal_chat_message_metadata_passthrough.as_ref();
        let kinds = metadata
            .and_then(|metadata| metadata.content_item_kinds.as_deref())
            .unwrap_or_default();
        let sources = metadata
            .and_then(|metadata| metadata.content_item_metadata.as_deref())
            .unwrap_or_default();
        let is_role = |index: usize, content: &ContentItem| {
            let kind = kinds.get(index).map_or("", |kind| kind.as_str());
            kind == MultiAgentRoleInstructions::KIND
                // Older rollouts could omit provenance. Preserve their known fork
                // boundary, not the stale role hints that shared its kind.
                || kind == MultiAgentUsageHint::KIND
                    && match sources.get(index).map(|source| &source.provenance) {
                        Some(ContentItemProvenance::Tool { .. }) => true,
                        None | Some(ContentItemProvenance::Unknown) => matches!(
                            content,
                            ContentItem::InputText { text }
                                if !text.starts_with("You are now a newly spawned child agent. ")
                        ),
                        Some(_) => false,
                    }
                || matches!(kind, "" | "unknown")
                    && matches!(content, ContentItem::InputText { text } if MultiAgentRoleInstructions::matches_text(text))
        };
        let removed: Vec<_> = content
            .iter()
            .enumerate()
            .filter_map(|(index, content)| is_role(index, content).then_some(index))
            .collect();
        if removed.is_empty() {
            return true;
        }
        if removed.len() == content.len() {
            return false;
        }
        let item = Arc::make_mut(item);
        let Some(content) = to_annotated_content(item) else {
            return true;
        };
        let retained = content
            .into_iter()
            .enumerate()
            .filter_map(|(index, part)| (!removed.contains(&index)).then_some(part))
            .collect();
        // A mixed message no longer has the original provider-visible content.
        item.set_id(None);
        set_annotated_content(item, retained).is_some()
    });
}

pub(super) fn usage_hint_text(step_context: &StepContext) -> Option<MultiAgentRoleInstructions> {
    let turn_context = step_context.turn.as_ref();
    if turn_context.multi_agent_version != MultiAgentVersion::V2
        || turn_context
            .config
            .subagent_collaboration_disabled(&turn_context.session_source)
    {
        return None;
    }

    let multi_agent_messages =
        ResolvedModelMessages::from_model(&step_context.settings.model_info).multi_agent();
    let snapshot = resolve_usage_hints(
        &turn_context.config.multi_agent_v2,
        multi_agent_messages,
        !turn_context.config.update_plan_enabled && turn_context.config.model_catalog.is_none(),
    );
    match &turn_context.session_source {
        SessionSource::SubAgent(SubAgentSource::ThreadSpawn { .. }) => snapshot.subagent,
        SessionSource::Cli
        | SessionSource::VSCode
        | SessionSource::Exec
        | SessionSource::Mcp
        | SessionSource::Custom(_)
        | SessionSource::Unknown => snapshot.root,
        SessionSource::Internal(_) | SessionSource::SubAgent(_) => None,
    }
}

pub(crate) fn resolve_usage_hints(
    config: &MultiAgentV2Config,
    multi_agent_messages: ResolvedMultiAgentMessages<'_>,
    omit_update_plan_instructions: bool,
) -> ResolvedMultiAgentV2UsageHints {
    let resolve_role = |configured: Option<&str>, message: ResolvedMessage<'_>| {
        // Configured roles take precedence; empty configured or catalog roles suppress fallback.
        if let Some(configured) = configured {
            return (!configured.is_empty())
                .then(|| MultiAgentRoleInstructions::Configured(configured.to_owned()));
        }

        let base = message.text();
        if base.is_empty() {
            return None;
        }
        if config.disable_native_tools {
            return Some(MultiAgentRoleInstructions::concurrency_only(
                config.max_concurrent_threads_per_session,
            ));
        }
        Some(MultiAgentRoleInstructions::Composed {
            base: base.to_owned(),
            omit_update_plan_instructions,
            max_concurrency: config.max_concurrent_threads_per_session,
            wait_agent_enabled: config.wait_agent_enabled,
            expose_model_overrides: config.expose_spawn_agent_model_overrides
                && !config.requires_spawn_agent_model(),
        })
    };

    ResolvedMultiAgentV2UsageHints {
        root: resolve_role(
            config.root_agent_usage_hint_text.as_deref(),
            multi_agent_messages.root,
        ),
        subagent: resolve_role(
            config.subagent_usage_hint_text.as_deref(),
            multi_agent_messages.subagent,
        ),
    }
}

pub(crate) fn effective_multi_agent_mode(step_context: &StepContext) -> Option<MultiAgentMode> {
    let turn_context = step_context.turn.as_ref();
    let settings = &step_context.settings;
    if turn_context.multi_agent_version != MultiAgentVersion::V2 {
        return None;
    }
    if turn_context
        .config
        .subagent_collaboration_disabled(&turn_context.session_source)
    {
        return Some(MultiAgentMode::Custom(
            "Collaboration tools are unavailable to this subagent. Finish the task yourself. Your final response goes to your parent agent."
                .to_string(),
        ));
    }

    let config = &turn_context.config.multi_agent_v2;
    let model_messages = if config.disable_native_tools {
        ResolvedModelMessages::bundled()
    } else {
        ResolvedModelMessages::from_model(&settings.model_info)
    };
    let multi_agent_messages = model_messages.multi_agent();
    let hint = config
        .multi_agent_mode_hint_text
        .as_deref()
        .or(multi_agent_messages.hint);
    let multi_agent_mode = match hint {
        Some(text) => MultiAgentMode::Custom(text.to_owned()),
        None => {
            let (message, builtin) =
                if settings.effective_reasoning_effort() == Some(ReasoningEffort::Ultra) {
                    (multi_agent_messages.proactive, MultiAgentMode::Proactive)
                } else {
                    (
                        multi_agent_messages.explicit,
                        MultiAgentMode::ExplicitRequestOnly,
                    )
                };
            if config.disable_native_tools {
                let policy = message
                    .text()
                    .replace("collaboration tools", "multi-agent tools");
                MultiAgentMode::Custom(format!(
                    "\n{policy}\n\n{HOSTED_MULTI_AGENT_SKILL_GUIDANCE}\n"
                ))
            } else {
                match message {
                    ResolvedMessage::Catalog(text) => MultiAgentMode::Custom(text.to_owned()),
                    ResolvedMessage::Bundled(_) => builtin,
                }
            }
        }
    };

    match &turn_context.session_source {
        SessionSource::SubAgent(SubAgentSource::ThreadSpawn { .. })
        | SessionSource::Cli
        | SessionSource::VSCode
        | SessionSource::Exec
        | SessionSource::Mcp
        | SessionSource::Custom(_)
        | SessionSource::Unknown => Some(multi_agent_mode),
        SessionSource::Internal(_) | SessionSource::SubAgent(_) => None,
    }
}
