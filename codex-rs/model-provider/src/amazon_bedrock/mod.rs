use crate::provider::ModelProvider;
use crate::provider::ModelProviderFuture;
use crate::provider::ProviderAccountResult;
use crate::provider::ProviderAccountState;
use codex_api::Provider;
use codex_api::SharedAuthProvider;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_models_manager::manager::SharedModelsManager;
use codex_models_manager::manager::StaticModelsManager;
use codex_protocol::account::ProviderAccount;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::openai_models::ModelsResponse;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct AmazonBedrockModelProvider {
    info: ModelProviderInfo,
    managed_credentials: bool,
}
impl AmazonBedrockModelProvider {
    pub(crate) fn new(info: ModelProviderInfo, auth_manager: Option<Arc<AuthManager>>) -> Self {
        let managed_credentials = auth_manager
            .and_then(|manager| manager.auth_cached())
            .is_some_and(|auth| {
                matches!(
                    auth,
                    CodexAuth::BedrockApiKey(_) | CodexAuth::BedrockAccessKeys(_)
                )
            })
            && info
                .aws
                .as_ref()
                .is_none_or(|aws| aws.profile.is_none() && aws.credential_export.is_none());
        Self {
            info,
            managed_credentials,
        }
    }
}
impl ModelProvider for AmazonBedrockModelProvider {
    fn capabilities(&self) -> crate::ProviderCapabilities {
        crate::ProviderCapabilities {
            reasoning_summaries: true,
            image_generation: false,
            web_search: !self.info.is_amazon_bedrock_runtime(),
            external_web_access: false,
            remote_compaction: crate::RemoteCompactionSupport::V2,
        }
    }
    fn info(&self) -> &ModelProviderInfo {
        &self.info
    }
    fn auth_manager(&self) -> Option<Arc<AuthManager>> {
        None
    }
    fn auth(&self) -> ModelProviderFuture<'_, Option<CodexAuth>> {
        Box::pin(async { None })
    }
    fn account_state(&self) -> ProviderAccountResult {
        Ok(ProviderAccountState {
            account: Some(ProviderAccount::AmazonBedrock {
                uses_codex_managed_credentials: self.managed_credentials,
            }),
            requires_openai_auth: false,
        })
    }
    fn api_provider_for_auth(&self, _auth: Option<&CodexAuth>) -> Result<Provider> {
        Err(CodexErr::UnsupportedOperation(
            "Amazon Bedrock is unsupported in this build.".into(),
        ))
    }
    fn api_auth(&self) -> ModelProviderFuture<'_, Result<SharedAuthProvider>> {
        Box::pin(async {
            Err(CodexErr::UnsupportedOperation(
                "Amazon Bedrock is unsupported in this build.".into(),
            ))
        })
    }
    fn runtime_base_url(&self) -> ModelProviderFuture<'_, Result<Option<String>>> {
        Box::pin(async {
            Err(CodexErr::UnsupportedOperation(
                "Amazon Bedrock is unsupported in this build.".into(),
            ))
        })
    }
    fn models_manager(
        &self,
        _codex_home: PathBuf,
        _catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        self.models_manager_without_cache(/*config_model_catalog*/ None)
    }
    fn models_manager_without_cache(
        &self,
        _catalog: Option<ModelsResponse>,
    ) -> SharedModelsManager {
        Arc::new(StaticModelsManager::new(
            /*auth_manager*/ None,
            ModelsResponse::default(),
            codex_models_manager::ModelOrigin::Bundled,
        ))
    }
}
pub fn is_supported_amazon_bedrock_region(region: &str) -> bool {
    matches!(
        region,
        "us-east-2"
            | "us-east-1"
            | "us-west-2"
            | "ap-southeast-3"
            | "ap-south-1"
            | "ap-northeast-1"
            | "eu-central-1"
            | "eu-west-1"
            | "eu-west-2"
            | "eu-south-1"
            | "eu-north-1"
            | "sa-east-1"
    ) || is_amazon_bedrock_gov_cloud_region(region)
}
pub fn is_amazon_bedrock_gov_cloud_region(region: &str) -> bool {
    matches!(region, "us-gov-east-1" | "us-gov-west-1")
}
pub async fn resolve_amazon_bedrock_region(
    _info: ModelProviderInfo,
    _auth: Option<Arc<AuthManager>>,
) -> Result<String> {
    Err(CodexErr::UnsupportedOperation(
        "Amazon Bedrock is unsupported in this build.".into(),
    ))
}
