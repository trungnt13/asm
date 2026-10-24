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
    catalog
});

pub fn provider_for_model(model: &str) -> Option<&'static str> {
    CATALOG
        .models
        .iter()
        .find(|entry| entry.model.slug == model)
        .and_then(|entry| entry.model_provider.as_deref())
}

pub(crate) fn models() -> impl Iterator<Item = &'static ModelInfo> {
    CATALOG.models.iter().map(|entry| &entry.model)
}
