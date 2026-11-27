use crate::manager::BUNDLED_MODELS;
use codex_protocol::config_types::ExternalModelRoute;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::is_native_runtime_model;
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashSet;
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
                instructions_template: Some(crate::model_info::BASE_INSTRUCTIONS.into()),
                ..Default::default()
            });
        }
    }
    catalog
});

static BUNDLED_ROUTES: LazyLock<BTreeMap<String, ExternalModelRoute>> = LazyLock::new(|| {
    let routes = CATALOG
        .models
        .iter()
        .map(|entry| {
            (
                entry.model.slug.clone(),
                ExternalModelRoute {
                    model_provider: entry.model_provider.clone(),
                    alias: entry.alias.clone(),
                },
            )
        })
        .collect();
    validate_routes(&routes).expect("bundled external model routes must be valid");
    routes
});

/// Exact routing and aliases for external model IDs.
#[derive(Debug, PartialEq, Eq)]
pub struct ExternalModels {
    routes: Cow<'static, BTreeMap<String, ExternalModelRoute>>,
    configured: HashSet<String>,
}

impl Default for ExternalModels {
    fn default() -> Self {
        Self {
            routes: Cow::Borrowed(&BUNDLED_ROUTES),
            configured: HashSet::new(),
        }
    }
}

impl ExternalModels {
    pub fn from_config(config: &BTreeMap<String, ExternalModelRoute>) -> Result<Self, String> {
        let mut models = Self::default();
        if !config.is_empty() {
            let routes = models.routes.to_mut();
            for (slug, route) in config {
                let entry = routes.entry(slug.clone()).or_default();
                if let Some(provider) = &route.model_provider {
                    entry.model_provider = Some(provider.clone());
                }
                if let Some(alias) = &route.alias {
                    entry.alias = Some(alias.clone());
                }
            }
            validate_routes(routes)?;
            models.configured.extend(config.keys().cloned());
        }
        Ok(models)
    }

    pub fn canonical_model(&self, model: &str) -> Option<&str> {
        self.routes.iter().find_map(|(slug, route)| {
            (route.alias.as_deref() == Some(model)).then_some(slug.as_str())
        })
    }

    pub fn provider_for_model(&self, model: &str) -> Option<&str> {
        self.routes.get(model)?.model_provider.as_deref()
    }

    pub fn provider_has_model_bindings(&self, provider: &str) -> bool {
        self.routes
            .values()
            .any(|route| route.model_provider.as_deref() == Some(provider))
    }

    pub fn model_alias(&self, model: &str) -> Option<&str> {
        self.routes.get(model)?.alias.as_deref()
    }

    pub fn is_configured(&self, model: &str) -> bool {
        self.configured.contains(model)
    }
}

fn validate_routes(routes: &BTreeMap<String, ExternalModelRoute>) -> Result<(), String> {
    let mut aliases = HashSet::new();
    for (slug, route) in routes {
        let slug = slug.as_str();
        let invalid = |message: &str| format!("external_models.{slug}: {message}");
        if slug.is_empty() || slug != slug.trim() {
            return Err(invalid("invalid model ID"));
        }
        match route.model_provider.as_deref() {
            Some(_) if is_native_runtime_model(slug) => {
                return Err(invalid(
                    "native runtime models cannot specify `model_provider`",
                ));
            }
            Some(provider) if provider.is_empty() || provider != provider.trim() => {
                return Err(invalid("invalid `model_provider`"));
            }
            None if !is_native_runtime_model(slug) => {
                return Err(invalid(
                    "non-native external models require `model_provider`",
                ));
            }
            Some(_) | None => {}
        }
        if let Some(alias) = route.alias.as_deref() {
            if alias.is_empty() || alias != alias.trim() {
                return Err(invalid("invalid `alias`"));
            }
            if routes.contains_key(alias)
                || is_native_runtime_model(alias)
                || BUNDLED_MODELS.iter().any(|model| model.slug == alias)
            {
                return Err(invalid(&format!(
                    "alias `{alias}` collides with a model ID"
                )));
            }
            if !aliases.insert(alias) {
                return Err(invalid(&format!("duplicate alias `{alias}`")));
            }
        }
    }
    Ok(())
}

// Bundled-only access for callers that have no configuration.
static DEFAULT_EXTERNAL_MODELS: LazyLock<ExternalModels> = LazyLock::new(ExternalModels::default);

pub fn provider_for_model(model: &str) -> Option<&'static str> {
    DEFAULT_EXTERNAL_MODELS.provider_for_model(model)
}

pub fn provider_has_model_bindings(provider: &str) -> bool {
    DEFAULT_EXTERNAL_MODELS.provider_has_model_bindings(provider)
}

pub fn bundled_provider_model(model: &str) -> Option<(&'static str, &'static ModelInfo)> {
    CATALOG
        .models
        .iter()
        .find(|entry| entry.model.slug == model)
        .and_then(|entry| Some((entry.model_provider.as_deref()?, &entry.model)))
}

pub fn bundled_model(model: &str) -> Option<&'static ModelInfo> {
    models().find(|info| info.slug == model)
}

pub(crate) fn models() -> impl Iterator<Item = &'static ModelInfo> {
    CATALOG.models.iter().map(|entry| &entry.model)
}

pub fn canonical_model(model: &str) -> Option<&'static str> {
    DEFAULT_EXTERNAL_MODELS.canonical_model(model)
}

pub fn model_alias(model: &str) -> Option<&'static str> {
    DEFAULT_EXTERNAL_MODELS.model_alias(model)
}
