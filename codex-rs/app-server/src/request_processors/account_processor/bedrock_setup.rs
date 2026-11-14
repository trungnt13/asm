use super::AccountRequestProcessor;
use crate::error_code::invalid_request;
use codex_app_server_protocol::BedrockDiscoverParams;
use codex_app_server_protocol::BedrockSetupParams;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;

impl AccountRequestProcessor {
    pub(crate) async fn bedrock_discover(
        &self,
        _params: BedrockDiscoverParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        Err(invalid_request(
            "Amazon Bedrock is unsupported in this build.",
        ))
    }
    pub(crate) async fn bedrock_setup(
        &self,
        _params: BedrockSetupParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        Err(invalid_request(
            "Amazon Bedrock is unsupported in this build.",
        ))
    }
}
