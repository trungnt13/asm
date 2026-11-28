use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use codex_async_utils::OrCancelExt;
use codex_features::Feature;
use codex_protocol::models::ResponseItem;
use tokio_util::sync::CancellationToken;

use crate::context::ContextualUserFragment;
use crate::context::DeveloperInstructions;
use crate::context::world_state::AdditionalContextState;
use crate::context::world_state::WorldState;
use crate::session::SubmittedAdditionalContext;
use crate::session::TurnInput;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::session::turn;
use crate::session::turn_context::TurnContext;

impl Session {
    pub(crate) fn render_native_additional_context(
        &self,
        section: AdditionalContextState,
    ) -> Vec<TurnInput> {
        let updates = {
            let mut baseline = self
                .external_runtime
                .additional_context_baseline
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (snapshot, updates) = WorldState::render_section(
                section,
                baseline.rendered.as_deref(),
                std::iter::empty(),
            );
            baseline.rendered = Some(Arc::new(snapshot));
            updates
        };
        self.annotated_additional_context_updates(updates)
            .map(TurnInput::ResponseItem)
            .collect()
    }
}

pub(super) fn submitted_additional_context(
    session: &Session,
    turn: &TurnContext,
    context: &SubmittedAdditionalContext,
) -> Vec<TurnInput> {
    let section = AdditionalContextState::new(
        &context.0,
        session.enabled(Feature::UntrustedAsToolOutput),
        &turn.session_telemetry,
        &session.context_ids,
    );
    session.render_native_additional_context(section)
}

pub(super) struct PreparedContext {
    pub(super) step: Arc<StepContext>,
    injections: Vec<ResponseItem>,
    connectors: HashSet<String>,
}

#[tracing::instrument(level = "trace", skip_all, err)]
pub(super) async fn capture(
    session: &Arc<Session>,
    turn: Arc<TurnContext>,
    input: &[TurnInput],
    restored_role: Option<&str>,
    cancellation: &CancellationToken,
) -> anyhow::Result<PreparedContext> {
    let user_input = turn::input_for_extensions(input);
    let (required_servers, mentioned_plugins) =
        turn::required_mcp_servers_for_input(session, &turn, &user_input)
            .or_cancel(cancellation)
            .await
            .map_err(|_| anyhow::anyhow!("ASM plugin preparation was cancelled"))?;
    let required_plugins = crate::plugins::collect_explicit_plugin_ids(&user_input);
    let step = session
        .capture_step_context_with_required_mcp_servers(
            turn,
            cancellation,
            &required_servers,
            &required_plugins,
        )
        .await?;
    let (mut injections, connectors) = turn::build_skills_and_plugins(
        session,
        &step,
        &user_input,
        &mentioned_plugins,
        cancellation,
    )
    .await
    .context("ASM skill and plugin preparation was cancelled")?;
    if let Some(restored_role) = restored_role {
        injections.insert(
            /*index*/ 0,
            ContextualUserFragment::into(DeveloperInstructions::new(restored_role)),
        );
    }
    Ok(PreparedContext {
        step,
        injections,
        connectors,
    })
}

#[tracing::instrument(level = "trace", skip_all, err)]
pub(super) async fn record<F>(
    session: &Session,
    prepared: PreparedContext,
    admit: impl FnOnce(&[ResponseItem], &[ResponseItem]) -> F + Send,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = anyhow::Result<()>> + Send,
{
    let PreparedContext {
        step,
        injections,
        connectors,
    } = prepared;
    session
        .record_context_updates_after(&step, |items| admit(items, &injections))
        .await?;
    session.merge_connector_selection(connectors).await;
    if !injections.is_empty() {
        session
            .record_conversation_items(&step.turn, &step.settings.model_info, &injections)
            .await;
    }
    Ok(())
}
