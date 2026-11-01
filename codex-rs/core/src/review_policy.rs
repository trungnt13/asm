use crate::config::Config;
use crate::config::Constrained;
use codex_features::Feature;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::models::BaseInstructionsProvenance;
use codex_protocol::protocol::AskForApproval;

pub(crate) fn apply_review_policy(config: &mut Config) {
    if let Err(error) = config.web_search_mode.set(WebSearchMode::Disabled) {
        panic!("by construction Constrained<WebSearchMode> must always support Disabled: {error}");
    }
    let _ = config.features.disable(Feature::Collab);
    let _ = config.features.disable(Feature::MultiAgentV2);
    config.base_instructions = Some(crate::REVIEW_PROMPT.to_string());
    config.base_instructions_provenance = Some(BaseInstructionsProvenance::Custom);
    config.permissions.approval_policy = Constrained::allow_only(AskForApproval::Never);
}
