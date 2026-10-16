use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

/// Decision totals for one loaded thread runtime, excluding warmup.
/// Totals reset when the runtime unloads; each child has separate totals.
/// Setup failures do not contribute completed attempts or elapsed time.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct System1Stats {
    pub completed_attempts: u64,
    pub accepted: u64,
    pub effort_changes: u64,
    pub errors: u64,
    pub ignored: u64,
    pub setup_failures: u64,
    pub total_elapsed_ms: u64,
    pub last_elapsed_ms: Option<u64>,
}
