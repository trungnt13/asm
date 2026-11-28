use codex_models_manager::ExternalModels;
use std::io;

pub(crate) fn validate_saved_model_provider(
    external_models: &ExternalModels,
    model: &str,
    provider: &str,
) -> io::Result<()> {
    if let Some(canonical) = external_models.canonical_model(model) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "Saved model `{model}` is now an alias for `{canonical}`. Restore its model ID or start a new thread."
            ),
        ));
    }
    if let Some(bound_provider) = external_models.provider_for_model(model)
        && bound_provider != provider
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "Saved model `{model}` uses provider `{provider}` but is now bound to `{bound_provider}`. Restore its provider binding or start a new thread."
            ),
        ));
    }
    Ok(())
}
