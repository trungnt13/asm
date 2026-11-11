use crate::error_code::internal_error;
use codex_app_server_protocol::ExternalAgentConfigImportHistory;
use codex_app_server_protocol::ExternalAgentConfigImportItemTypeFailure as ProtocolImportFailure;
use codex_app_server_protocol::ExternalAgentConfigImportItemTypeSuccess as ProtocolImportSuccess;
use codex_app_server_protocol::ExternalAgentConfigMigrationItemType;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_state::ExternalAgentConfigImportFailureRecord;
use codex_state::ExternalAgentConfigImportSuccessRecord;

pub(super) fn protocol_import_history(
    record: codex_state::ExternalAgentConfigImportHistoryRecord,
) -> Result<ExternalAgentConfigImportHistory, JSONRPCErrorError> {
    let successes = record
        .successes
        .into_iter()
        .map(protocol_import_success_record)
        .collect::<Result<Vec<_>, _>>()?;
    let failures = record
        .failures
        .into_iter()
        .map(protocol_import_failure_record)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ExternalAgentConfigImportHistory {
        import_id: record.import_id,
        provider_id: record.provider_id,
        completed_at_ms: record.completed_at_ms,
        successes,
        failures,
    })
}

fn protocol_import_success_record(
    record: ExternalAgentConfigImportSuccessRecord,
) -> Result<ProtocolImportSuccess, JSONRPCErrorError> {
    Ok(ProtocolImportSuccess {
        item_type: protocol_import_record_item_type(record.item_type)?,
        cwd: record.cwd,
        source: record.source,
        target: record.target,
        title: record.title,
    })
}

fn protocol_import_failure_record(
    record: ExternalAgentConfigImportFailureRecord,
) -> Result<ProtocolImportFailure, JSONRPCErrorError> {
    Ok(ProtocolImportFailure {
        item_type: protocol_import_record_item_type(record.item_type)?,
        error_type: record.error_type,
        sub_error_type: record.sub_error_type,
        failure_stage: record.failure_stage,
        message: record.message,
        cwd: record.cwd,
        source: record.source,
    })
}

fn protocol_import_record_item_type(
    item_type: String,
) -> Result<ExternalAgentConfigMigrationItemType, JSONRPCErrorError> {
    serde_json::from_value(serde_json::Value::String(item_type.clone())).map_err(|err| {
        internal_error(format!(
            "failed to decode import item type {item_type}: {err}"
        ))
    })
}
