use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum CompanionRelationship {
    Side,
    Chat,
    Parallel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(tag = "type", export_to = "v2/")]
pub enum CompanionLifecycleEvent {
    #[serde(rename_all = "camelCase")]
    Opened {
        session_id: String,
        relationship: CompanionRelationship,
    },
    #[serde(rename_all = "camelCase")]
    Closed { session_id: String },
    #[serde(rename_all = "camelCase")]
    Replaced {
        old_session_id: String,
        new_session_id: String,
        relationship: CompanionRelationship,
    },
}
