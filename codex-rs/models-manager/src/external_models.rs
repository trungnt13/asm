use codex_protocol::openai_models::ModelInfo;
use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Catalog {
    models: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    model_provider: Option<String>,
    alias: Option<String>,
    #[serde(flatten)]
    model: ModelInfo,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    let mut catalog: Catalog = serde_json::from_str(include_str!("../external_models.json"))
        .expect("bundled external model catalog must be valid");
    for entry in &mut catalog.models {
        if entry.model_provider.is_some() && entry.model.model_messages.is_none() {
            entry.model.model_messages = Some(codex_protocol::openai_models::ModelMessages {
                instructions_template: Some(crate::model_info::BASE_INSTRUCTIONS.to_owned()),
                ..Default::default()
            });
        }
    }
    for entry in &catalog.models {
        if let Some(alias) = entry.alias.as_deref() {
            assert!(
                entry.model_provider.is_some(),
                "model aliases require a provider"
            );
            assert!(!alias.is_empty(), "model aliases must not be empty");
            assert!(
                !catalog.models.iter().any(|other| other.model.slug == alias),
                "model alias collides with a model ID"
            );
            assert_eq!(
                catalog
                    .models
                    .iter()
                    .filter(|other| other.alias.as_deref() == Some(alias))
                    .count(),
                1,
                "duplicate model alias"
            );
        }
    }
    catalog
});

pub fn provider_for_model(model: &str) -> Option<&'static str> {
    CATALOG
        .models
        .iter()
        .find(|entry| entry.model.slug == model)
        .and_then(|entry| entry.model_provider.as_deref())
}

pub fn provider_has_model_bindings(provider: &str) -> bool {
    CATALOG
        .models
        .iter()
        .any(|entry| entry.model_provider.as_deref() == Some(provider))
}

pub(crate) fn models() -> impl Iterator<Item = &'static ModelInfo> {
    CATALOG.models.iter().map(|entry| &entry.model)
}

pub fn canonical_model(model: &str) -> Option<&'static str> {
    CATALOG
        .models
        .iter()
        .find(|entry| entry.alias.as_deref() == Some(model))
        .map(|entry| entry.model.slug.as_str())
}

pub fn model_alias(model: &str) -> Option<&'static str> {
    CATALOG
        .models
        .iter()
        .find(|entry| entry.model.slug == model)
        .and_then(|entry| entry.alias.as_deref())
}
