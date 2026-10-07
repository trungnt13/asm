use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

use crate::ThreadId;

/// Persisted identity of a child thread executed by an external runtime.
///
/// Hosts must restore this identity before dispatching resumed input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ExternalAgentDescriptor {
    pub version: u32,
    pub backend_id: String,
    pub runtime_session_id: ThreadId,
    pub model: String,
}
