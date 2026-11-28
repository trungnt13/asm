use super::AccountRequestProcessor;
use crate::error_code::internal_error;
use codex_app_server_protocol::BedrockCheckGovCloudRequirementsResponse;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;

impl AccountRequestProcessor {
    pub(crate) async fn bedrock_check_gov_cloud_requirements(
        &self,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.config_manager
            .load_latest_config(/*fallback_cwd*/ None)
            .await
            .map_err(|err| internal_error(format!("failed to load configuration: {err}")))?;
        Ok(Some(
            BedrockCheckGovCloudRequirementsResponse {
                is_gov_cloud: false,
                should_warn: false,
            }
            .into(),
        ))
    }
}
