use super::connectors::read_imported_connector_candidates;
use super::protocol::protocol_import_history;
use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use crate::outgoing_message::ConnectionRequestId;
use codex_app_server_protocol::ExternalAgentConfigDetectParams;
use codex_app_server_protocol::ExternalAgentConfigDetectResponse;
use codex_app_server_protocol::ExternalAgentConfigImportHistoriesReadResponse;
use codex_app_server_protocol::ExternalAgentConfigImportHistoryRecordParams;
use codex_app_server_protocol::ExternalAgentConfigImportHistoryRecordResponse;
use codex_app_server_protocol::ExternalAgentConfigImportItemTypeSuccess as ProtocolImportSuccess;
use codex_app_server_protocol::ExternalAgentConfigImportParams;
use codex_app_server_protocol::ExternalAgentConfigImportTypeResult as ProtocolImportTypeResult;
use codex_app_server_protocol::ExternalAgentImportedConnectorCandidate;
use codex_app_server_protocol::ExternalAgentImportedConnectorSource;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_rollout::StateDbHandle;
use codex_state::ExternalAgentConfigImportFailureRecord;
use codex_state::ExternalAgentConfigImportSuccessRecord;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct ExternalAgentConfigRequestProcessor {
    state_db: Option<StateDbHandle>,
    codex_home: PathBuf,
}
pub(crate) struct ExternalAgentConfigRequestProcessorArgs {
    pub(crate) state_db: Option<StateDbHandle>,
    pub(crate) codex_home: PathBuf,
}
impl ExternalAgentConfigRequestProcessor {
    pub(crate) fn new(args: ExternalAgentConfigRequestProcessorArgs) -> Self {
        Self {
            state_db: args.state_db,
            codex_home: args.codex_home,
        }
    }
    pub(crate) async fn detect(
        &self,
        _params: ExternalAgentConfigDetectParams,
    ) -> Result<ExternalAgentConfigDetectResponse, JSONRPCErrorError> {
        Err(invalid_request(
            "external-agent configuration migration is not included in this ASM build",
        ))
    }
    pub(crate) async fn import(
        &self,
        _request_id: ConnectionRequestId,
        _params: ExternalAgentConfigImportParams,
    ) -> Result<(), JSONRPCErrorError> {
        Err(invalid_request(
            "external-agent configuration migration is not included in this ASM build",
        ))
    }

    pub(crate) async fn read_import_histories(
        &self,
    ) -> Result<ExternalAgentConfigImportHistoriesReadResponse, JSONRPCErrorError> {
        let state_db = self
            .state_db
            .as_ref()
            .ok_or_else(|| internal_error("state database is unavailable"))?;
        let histories = state_db
            .external_agent_config_import_history_records()
            .await
            .map_err(|err| internal_error(format!("failed to read import histories: {err}")))?;
        let data = histories
            .into_iter()
            .map(protocol_import_history)
            .collect::<Result<Vec<_>, _>>()?;
        let connectors = read_imported_connector_candidates(&self.codex_home)
            .map_err(|err| {
                internal_error(format!(
                    "failed to read imported connector candidates: {err}"
                ))
            })?
            .into_iter()
            .map(|candidate| ExternalAgentImportedConnectorCandidate {
                name: candidate.name,
                session_count: candidate.session_count,
                source: ExternalAgentImportedConnectorSource::RemoteMcpServersConfig,
            })
            .collect();

        Ok(ExternalAgentConfigImportHistoriesReadResponse { data, connectors })
    }

    pub(crate) async fn record_import_history(
        &self,
        params: ExternalAgentConfigImportHistoryRecordParams,
    ) -> Result<ExternalAgentConfigImportHistoryRecordResponse, JSONRPCErrorError> {
        let state_db = self
            .state_db
            .as_ref()
            .ok_or_else(|| internal_error("state database is unavailable"))?;
        let import_id = Uuid::new_v4().to_string();
        let item_type_results = params
            .item_type_results
            .into_iter()
            .map(|type_result| ProtocolImportTypeResult {
                item_type: type_result.item_type,
                successes: type_result
                    .successes
                    .into_iter()
                    .map(|success| ProtocolImportSuccess {
                        item_type: success.item_type,
                        cwd: success.cwd,
                        source: success.source,
                        target: success.target,
                        title: success.title,
                    })
                    .collect(),
                failures: type_result.failures,
            })
            .collect::<Vec<_>>();
        record_import_history(
            state_db,
            import_id.as_str(),
            Some(params.provider_id.as_str()),
            &item_type_results,
        )
        .await
        .map_err(|err| internal_error(format!("failed to record import history: {err}")))?;

        Ok(ExternalAgentConfigImportHistoryRecordResponse { import_id })
    }
}
async fn record_import_history(
    state_db: &StateDbHandle,
    import_id: &str,
    provider_id: Option<&str>,
    item_type_results: &[ProtocolImportTypeResult],
) -> anyhow::Result<()> {
    let successes = item_type_results
        .iter()
        .flat_map(|type_result| type_result.successes.iter())
        .map(|success| {
            Ok(ExternalAgentConfigImportSuccessRecord {
                item_type: serde_json::from_value(serde_json::to_value(success.item_type)?)?,
                cwd: success.cwd.clone(),
                source: success.source.clone(),
                target: success.target.clone(),
                title: success.title.clone(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let failures = item_type_results
        .iter()
        .flat_map(|type_result| type_result.failures.iter())
        .map(|failure| {
            Ok(ExternalAgentConfigImportFailureRecord {
                item_type: serde_json::from_value(serde_json::to_value(failure.item_type)?)?,
                error_type: failure.error_type.clone(),
                sub_error_type: failure.sub_error_type.clone(),
                failure_stage: failure.failure_stage.clone(),
                message: failure.message.clone(),
                cwd: failure.cwd.clone(),
                source: failure.source.clone(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    state_db
        .record_external_agent_config_import_completed(
            import_id,
            provider_id,
            &successes,
            &failures,
        )
        .await
}
