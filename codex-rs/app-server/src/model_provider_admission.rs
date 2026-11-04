use codex_models_manager::canonical_model;
use codex_models_manager::provider_for_model;
use codex_protocol::openai_models::is_native_runtime_model;

/// Keeps the model and provider from one saved chat identity together.
#[derive(Clone, Copy)]
pub(crate) struct SavedModelProvider<'a> {
    pub(crate) model: Option<&'a str>,
    pub(crate) provider: &'a str,
}

pub(super) fn model_selects_provider(model: Option<&str>, provider: &str) -> bool {
    let Some(model) = model else {
        return false;
    };
    let model = canonical_model(model).unwrap_or(model);
    is_native_runtime_model(model) || provider_for_model(model) == Some(provider)
}
