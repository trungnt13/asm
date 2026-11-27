use crate::manager::BUNDLED_MODELS;
use crate::model_info;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelMessages;
use codex_protocol::openai_models::ModelOrigin;
use codex_protocol::openai_models::ToolMode;
use codex_protocol::openai_models::is_native_runtime_model;
use codex_protocol::protocol::MultiAgentVersion;
use serde::Deserialize;
use serde::Deserializer;
use serde::de;
use serde_json::Map;
use serde_json::Value;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Catalog {
    models: Vec<Entry>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct Entry {
    model_provider: Option<String>,
    alias: Option<String>,
    #[serde(skip)]
    metadata_patch: Option<BTreeMap<String, Value>>,
    // Local metadata with the patch applied, for reads without a selected catalog entry.
    #[serde(flatten)]
    model: ModelInfo,
}

impl Entry {
    fn overlay(&self, existing: Option<&ModelInfo>, mode: OverlayMode) -> Option<ModelInfo> {
        let Some(fields) = &self.metadata_patch else {
            return match mode {
                OverlayMode::PreferBundled => Some(self.model.clone()),
                OverlayMode::SupplementCustom if existing.is_none() => Some(self.model.clone()),
                OverlayMode::ProviderOwned
                | OverlayMode::ConfiguredOnly
                | OverlayMode::SupplementCustom => None,
            };
        };
        let existing = existing.filter(|_| !self.prefers_bundled_external(mode));
        let Some(base) = existing else {
            return match mode {
                OverlayMode::ProviderOwned => None,
                OverlayMode::ConfiguredOnly
                | OverlayMode::SupplementCustom
                | OverlayMode::PreferBundled => Some(self.model.clone()),
            };
        };
        if fields.is_empty() {
            return None;
        }
        let mut model = match patch_metadata(base, fields) {
            Ok(model) => model,
            Err(error) => {
                // A selected catalog may lack optional objects the patch augments in the bundle.
                tracing::warn!(model = %base.slug, %error, "external model metadata patch does not apply to the selected catalog entry");
                return Some(base.clone());
            }
        };
        if matches!(mode, OverlayMode::ProviderOwned) {
            model.supported_in_api = base.supported_in_api;
            model.visibility = base.visibility;
            model
                .available_access_programs
                .clone_from(&base.available_access_programs);
        }
        Some(model)
    }

    fn prefers_bundled_external(&self, mode: OverlayMode) -> bool {
        matches!(mode, OverlayMode::PreferBundled) && self.has_bundled_external_base()
    }

    fn has_bundled_external_base(&self) -> bool {
        BUNDLED
            .models
            .iter()
            .any(|entry| entry.model.slug == self.model.slug)
    }
}

static BUNDLED: LazyLock<Catalog> = LazyLock::new(|| {
    let mut catalog: Catalog = serde_json::from_str(include_str!("../external_models.json"))
        .expect("bundled external model catalog must be valid");
    for entry in &mut catalog.models {
        if entry.model_provider.is_some() && entry.model.model_messages.is_none() {
            entry.model.model_messages = Some(ModelMessages {
                instructions_template: Some(model_info::BASE_INSTRUCTIONS.into()),
                ..Default::default()
            });
        }
    }
    validate_entries(&catalog.models).expect("bundled external model catalog must be valid");
    catalog
});

/// Local external model metadata and exact routing bindings for one configuration.
#[derive(Debug, PartialEq, Eq)]
pub struct ExternalModels {
    entries: Cow<'static, [Entry]>,
}

impl Default for ExternalModels {
    fn default() -> Self {
        Self {
            entries: Cow::Borrowed(&BUNDLED.models),
        }
    }
}

impl ExternalModels {
    pub(crate) fn bundled() -> &'static Self {
        &DEFAULT_EXTERNAL_MODELS
    }

    pub fn from_config(config: &BTreeMap<String, BTreeMap<String, Value>>) -> Result<Self, String> {
        if config.is_empty() {
            return Ok(Self::default());
        }
        let mut entries = BUNDLED.models.clone();
        for (slug, fields) in config {
            let index = entries.iter().position(|entry| entry.model.slug == *slug);
            let entry = configured_entry(slug, fields, index.map(|index| &entries[index]))
                .map_err(|err| format!("external_models.{slug}: {err}"))?;
            if let Some(index) = index {
                entries[index] = entry;
            } else {
                entries.push(entry);
            }
        }
        validate_entries(&entries)?;
        Ok(Self {
            entries: Cow::Owned(entries),
        })
    }

    pub fn canonical_model(&self, model: &str) -> Option<&str> {
        self.entries.iter().find_map(|entry| {
            (entry.alias.as_deref() == Some(model)).then_some(entry.model.slug.as_str())
        })
    }

    pub fn provider_for_model(&self, model: &str) -> Option<&str> {
        self.provider_model(model).map(|(provider, _)| provider)
    }

    /// Return the exact provider binding and local metadata before any provider catalog merge.
    pub fn provider_model(&self, model: &str) -> Option<(&str, &ModelInfo)> {
        let entry = self.entry(model)?;
        Some((entry.model_provider.as_deref()?, &entry.model))
    }

    pub fn provider_has_model_bindings(&self, provider: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.model_provider.as_deref() == Some(provider))
    }

    pub fn model_alias(&self, model: &str) -> Option<&str> {
        self.entry(model)?.alias.as_deref()
    }

    pub fn is_configured(&self, model: &str) -> bool {
        self.entry(model)
            .is_some_and(|entry| entry.metadata_patch.is_some())
    }

    pub(crate) fn overlay_origin(
        &self,
        model: &str,
        mode: OverlayMode,
        selected_origin: Option<ModelOrigin>,
    ) -> Option<ModelOrigin> {
        let entry = self.entry(model)?;
        if let Some(fields) = &entry.metadata_patch {
            if fields.is_empty() {
                if !entry.prefers_bundled_external(mode) && selected_origin.is_some() {
                    return selected_origin;
                }
                if entry.has_bundled_external_base()
                    || BUNDLED_MODELS.iter().any(|entry| entry.slug == model)
                {
                    return Some(ModelOrigin::Bundled);
                }
            }
            Some(ModelOrigin::ExplicitConfig)
        } else if matches!(mode, OverlayMode::PreferBundled) {
            Some(ModelOrigin::Bundled)
        } else {
            None
        }
    }

    pub(crate) fn provider_models<'a>(
        &'a self,
        provider: &'a str,
    ) -> impl Iterator<Item = &'a ModelInfo> {
        self.entries.iter().filter_map(move |entry| {
            (entry.model_provider.as_deref() == Some(provider)).then_some(&entry.model)
        })
    }

    pub(crate) fn overlay_exact(
        &self,
        slug: &str,
        existing: Option<&ModelInfo>,
        mode: OverlayMode,
    ) -> Option<ModelInfo> {
        self.entry(slug)?.overlay(existing, mode)
    }

    pub(crate) fn overlay(&self, models: &mut Cow<'_, [ModelInfo]>, mode: OverlayMode) {
        let overlay: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let existing = models.iter().find(|model| model.slug == entry.model.slug);
                entry.overlay(existing, mode)
            })
            .collect();
        if !overlay.is_empty() {
            let models = models.to_mut();
            models.retain(|model| !overlay.iter().any(|entry| entry.slug == model.slug));
            models.extend(overlay);
        }
    }

    fn entry(&self, slug: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.model.slug == slug)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum OverlayMode {
    ProviderOwned,
    ConfiguredOnly,
    SupplementCustom,
    PreferBundled,
}

fn configured_entry(
    slug: &str,
    fields: &BTreeMap<String, Value>,
    bundled: Option<&Entry>,
) -> Result<Entry, String> {
    if fields.contains_key("slug") {
        return Err("the model ID must be the table key, not a `slug` field".to_owned());
    }
    let mut fields = fields.clone();
    let read_binding = |field: &str, value: Option<Value>, default: Option<&String>| {
        value.map_or_else(
            || Ok(default.cloned()),
            |value| {
                serde_json::from_value(value).map_err(|err| format!("invalid `{field}`: {err}"))
            },
        )
    };
    let alias = read_binding(
        "alias",
        fields.remove("alias"),
        bundled.and_then(|entry| entry.alias.as_ref()),
    )?;
    let model_provider = read_binding(
        "model_provider",
        fields.remove("model_provider"),
        bundled.and_then(|entry| entry.model_provider.as_ref()),
    )?;

    // Remote catalogs ignore new selector values for forward compatibility. Local typos are errors.
    if let Some(value) = fields.get("tool_mode") {
        serde_json::from_value::<Option<ToolMode>>(value.clone())
            .map_err(|err| format!("invalid `tool_mode`: {err}"))?;
    }
    if let Some(value) = fields.get("multi_agent_version") {
        serde_json::from_value::<Option<MultiAgentVersion>>(value.clone())
            .map_err(|err| format!("invalid `multi_agent_version`: {err}"))?;
    }

    let mut model = bundled
        .map(|entry| &entry.model)
        .or_else(|| BUNDLED_MODELS.iter().find(|model| model.slug == slug))
        .cloned()
        .unwrap_or_else(|| model_info::configured_model_defaults(slug));
    if !fields.is_empty() {
        model = patch_metadata(&model, &fields).map_err(|err| err.to_string())?;
    }
    Ok(Entry {
        model_provider,
        alias,
        metadata_patch: Some(fields),
        model,
    })
}

fn patch_metadata(
    base: &ModelInfo,
    fields: &BTreeMap<String, Value>,
) -> Result<ModelInfo, serde_json::Error> {
    let Value::Object(mut metadata) = serde_json::to_value(base)? else {
        unreachable!("ModelInfo must serialize to a JSON object");
    };
    merge_metadata(&mut metadata, fields);
    ModelInfo::deserialize(StrictModelFields(metadata))
}

fn merge_metadata<'a>(
    metadata: &mut Map<String, Value>,
    fields: impl IntoIterator<Item = (&'a String, &'a Value)>,
) {
    for (key, value) in fields {
        match (metadata.get_mut(key), value) {
            (Some(Value::Object(metadata)), Value::Object(fields)) => {
                merge_metadata(metadata, fields);
            }
            (_, value) => {
                metadata.insert(key.clone(), value.clone());
            }
        }
    }
}

// Validate local field names without changing the permissive provider catalog format.
struct StrictModelFields(Map<String, Value>);

impl<'de> Deserializer<'de> for StrictModelFields {
    type Error = serde_json::Error;

    fn deserialize_struct<V: de::Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if let Some(unknown) = self
            .0
            .keys()
            .find(|field| !fields.contains(&field.as_str()))
        {
            return Err(de::Error::unknown_field(unknown, fields));
        }
        Value::Object(self.0).deserialize_struct(name, fields, visitor)
    }

    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        Value::Object(self.0).deserialize_any(visitor)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier ignored_any
    }
}

fn validate_entries(entries: &[Entry]) -> Result<(), String> {
    let canonical: HashSet<_> = entries
        .iter()
        .map(|entry| entry.model.slug.as_str())
        .collect();
    let mut aliases = HashSet::new();
    for entry in entries {
        let slug = entry.model.slug.as_str();
        let invalid = |message: &str| format!("external_models.{slug}: {message}");
        if slug.is_empty() || slug != slug.trim() {
            return Err(invalid("invalid model ID"));
        }
        match entry.model_provider.as_deref() {
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
        if let Some(alias) = entry.alias.as_deref() {
            if alias.is_empty() || alias != alias.trim() {
                return Err(invalid("invalid `alias`"));
            }
            if canonical.contains(alias)
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

#[cfg(test)]
pub(crate) fn models() -> impl Iterator<Item = &'static ModelInfo> {
    BUNDLED.models.iter().map(|entry| &entry.model)
}

pub fn canonical_model(model: &str) -> Option<&'static str> {
    DEFAULT_EXTERNAL_MODELS.canonical_model(model)
}

pub fn model_alias(model: &str) -> Option<&'static str> {
    DEFAULT_EXTERNAL_MODELS.model_alias(model)
}
