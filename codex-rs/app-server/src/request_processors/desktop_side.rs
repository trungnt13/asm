use codex_core::config::Config;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
// Desktop 26.1027.51404 supplies these complete blocks, but no structured side marker.
// Unknown client wording deliberately retains ordinary thread behavior.
const DEVELOPER_BLOCK: &str = include_str!("desktop_side_instructions.txt");
const BOUNDARY: &str = include_str!("desktop_side_boundary.txt");

pub(super) fn apply_defaults(
    instructions: Option<&str>,
    config: &mut Config,
    origin: SideOrigin,
) -> Option<SideSettingsInitialization> {
    let Some(instructions) = instructions else {
        return None;
    };
    let matches = instructions
        .match_indices(DEVELOPER_BLOCK)
        .any(|(offset, block)| {
            let end = offset + block.len();
            (offset == 0 || instructions[..offset].ends_with("\n\n"))
                && (end == instructions.len() || instructions[end..].starts_with("\n\n"))
        });
    if !matches {
        return None;
    }
    let configured = config.side_model.is_some()
        || config.side_reasoning_effort.is_some()
        || config.side_service_tier.is_some();
    if !configured {
        return None;
    }
    let initialization = SideSettingsInitialization {
        inherited_model: config.model.clone(),
        inherited_effort: config.model_reasoning_effort.clone(),
        inherited_tier: config.service_tier.clone(),
        normalize_model: config.side_model.is_some(),
        normalize_effort: config.side_reasoning_effort.is_some(),
        normalize_tier: config.side_service_tier.is_some(),
        pending_sync: true,
        pending_first_turn: matches!(origin, SideOrigin::Start),
    };
    if let Some(model) = &config.side_model {
        config.model = Some(model.clone());
    }
    if let Some(effort) = config.side_reasoning_effort.clone() {
        config.model_reasoning_effort = Some(effort);
    }
    if let Some(tier) = &config.side_service_tier {
        config.service_tier = Some(
            codex_protocol::config_types::ServiceTier::from_request_value(tier)
                .map(|tier| tier.request_value().to_string())
                .unwrap_or_else(|| tier.clone()),
        );
    }
    Some(initialization)
}

pub(super) fn is_boundary(items: &[ResponseItem]) -> bool {
    matches!(items, [ResponseItem::Message { role, content, .. }]
        if role == "user" && matches!(content.as_slice(),
            [ContentItem::InputText { text }] if text == BOUNDARY))
}

pub(super) enum SideOrigin {
    Start,
    Fork,
}

pub(crate) struct SideSettingsInitialization {
    inherited_model: Option<String>,
    inherited_effort: Option<codex_protocol::openai_models::ReasoningEffort>,
    pub(super) inherited_tier: Option<String>,
    pub(super) normalize_model: bool,
    pub(super) normalize_effort: bool,
    pub(super) normalize_tier: bool,
    pub(super) pending_sync: bool,
    pub(super) pending_first_turn: bool,
}

impl SideSettingsInitialization {
    pub(super) fn normalize_first_turn(
        &self,
        params: &mut codex_app_server_protocol::TurnStartParams,
        snapshot: &codex_core::ThreadConfigSnapshot,
    ) {
        if !self.pending_first_turn {
            return;
        }
        if let Some(mode) = params.collaboration_mode.as_mut() {
            if self.normalize_model && Some(&mode.settings.model) == self.inherited_model.as_ref() {
                mode.settings.model = snapshot.model.clone();
            }
            if self.normalize_effort
                && (mode.settings.reasoning_effort.is_none()
                    || mode.settings.reasoning_effort == self.inherited_effort)
            {
                mode.settings.reasoning_effort = snapshot.reasoning_effort.clone();
            }
        }
        if self.normalize_model && params.model.is_some() && params.model == self.inherited_model {
            params.model = Some(snapshot.model.clone());
        }
        if self.normalize_effort
            && params.effort.is_some()
            && params.effort == self.inherited_effort
        {
            params.effort = snapshot.reasoning_effort.clone();
        }
        if self.normalize_tier
            && let Some(tier) = &params.service_tier
            && tier.as_deref().unwrap_or("default")
                == self.inherited_tier.as_deref().unwrap_or("default")
        {
            params.service_tier = Some(snapshot.service_tier.clone());
        }
    }
}
