use std::num::NonZeroU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_login::auth::AgentIdentityAuthPolicy;
use codex_login::auth::BedrockApiKeyAuth;
use codex_model_provider_info::ModelProviderAwsAuthInfo;
use codex_model_provider_info::WireApi;
use codex_model_provider_info::create_oss_provider_with_base_url;
use codex_models_manager::manager::RefreshStrategy;
use codex_protocol::account::PlanType;
use codex_protocol::config_types::ModelProviderAuthInfo;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::protocol::SessionSource;
use codex_utils_redacted_string::RedactedString;
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header_regex;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::*;
use crate::RemoteCompactionSupport;
use crate::auth::AgentIdentitySessionFallback;

fn provider_info_with_command_auth() -> ModelProviderInfo {
    ModelProviderInfo {
        auth: Some(ModelProviderAuthInfo {
            command: "print-token".to_string(),
            args: Vec::new(),
            timeout_ms: NonZeroU64::new(5_000).expect("timeout should be non-zero"),
            refresh_interval_ms: 300_000,
            cwd: std::env::current_dir()
                .expect("current dir should be available")
                .try_into()
                .expect("current dir should be absolute"),
        }),
        requires_openai_auth: false,
        ..ModelProviderInfo::create_openai_provider(/*base_url*/ None)
    }
}

fn test_codex_home() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("codex-model-provider-test-{}", std::process::id()))
}

fn provider_for(base_url: String) -> ModelProviderInfo {
    ModelProviderInfo {
        name: "mock".into(),
        base_url: Some(base_url),
        model_catalog_url: None,
        env_key: None,
        env_key_instructions: None,
        experimental_bearer_token: None,
        auth: None,
        gateway_oauth: None,
        aws: None,
        wire_api: WireApi::Responses,
        query_params: None,
        http_headers: None,
        env_http_headers: None,
        request_max_retries: Some(0),
        stream_max_retries: Some(0),
        stream_idle_timeout_ms: Some(5_000),
        websocket_connect_timeout_ms: None,
        requires_openai_auth: false,
        supports_websockets: false,
        supports_standalone_web_search: false,
        standalone_web_search_provider: None,
        capabilities: None,
        project_id: None,
        location: None,
        credential_file: None,
        include_internal_metadata: false,
    }
}

fn remote_model(slug: &str) -> ModelInfo {
    serde_json::from_value(json!({
        "slug": slug,
        "display_name": slug,
        "description": null,
        "default_reasoning_level": "medium",
        "supported_reasoning_levels": [],
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 0,
        "upgrade": null,
        "support_verbosity": false,
        "default_verbosity": null,
        "apply_patch_tool_type": null,
        "truncation_policy": {"mode": "bytes", "limit": 10_000},
        "supports_image_detail_original": false,
        "context_window": 272_000,
        "max_context_window": 272_000,
        "experimental_supported_tools": [],
    }))
    .expect("valid model")
}

fn bedrock_api_key_auth() -> CodexAuth {
    CodexAuth::BedrockApiKey(BedrockApiKeyAuth {
        api_key: "bedrock-api-key-test".to_string(),
        region: "us-east-1".to_string(),
    })
}

#[tokio::test]
async fn scoped_auth_ignores_scope_for_non_openai_provider() {
    let provider = create_model_provider(
        create_oss_provider_with_base_url("http://localhost:11434/v1", WireApi::Responses),
        /*auth_manager*/ None,
    );

    let auth = provider
        .api_auth_for_scope(ProviderAuthScope {
            agent_identity_policy: AgentIdentityAuthPolicy::JwtOnly,
            session_source: SessionSource::Cli,
            agent_identity_session_fallback: AgentIdentitySessionFallback::default(),
        })
        .await
        .expect("auth should resolve");

    assert!(auth.auth.to_auth_headers().unwrap().is_empty());
}

#[test]
fn openai_provider_enables_remote_compaction() {
    let provider = create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        /*auth_manager*/ None,
    );

    assert_eq!(
        provider.capabilities(),
        ProviderCapabilities {
            remote_compaction: RemoteCompactionSupport::V2,
            ..ProviderCapabilities::default()
        }
    );
}

#[test]
fn configured_provider_remote_compaction_matches_provider_support() {
    let cases = [
        (
            ModelProviderInfo::create_openai_provider(/*base_url*/ None),
            RemoteCompactionSupport::V2,
        ),
        (
            ModelProviderInfo {
                name: "Azure".to_string(),
                base_url: Some("https://example.com/openai".to_string()),
                ..ModelProviderInfo::default()
            },
            RemoteCompactionSupport::V2,
        ),
        (
            ModelProviderInfo {
                name: "Custom".to_string(),
                base_url: Some("https://example.openai.azure.com/openai/v1".to_string()),
                ..ModelProviderInfo::default()
            },
            RemoteCompactionSupport::V2,
        ),
        (
            provider_for("https://example.test/v1".to_string()),
            RemoteCompactionSupport::Unsupported,
        ),
    ];

    for (provider_info, expected) in cases {
        let provider = create_model_provider(provider_info, /*auth_manager*/ None);
        assert_eq!(provider.capabilities().remote_compaction, expected);
    }
}

#[test]
fn configured_provider_uses_default_approval_review_preferred_model() {
    let provider = create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        /*auth_manager*/ None,
    );

    assert_eq!(
        provider.approval_review_preferred_model(),
        DEFAULT_APPROVAL_REVIEW_PREFERRED_MODEL
    );
}

#[test]
fn configured_provider_uses_luna_for_approval_review_with_api_key_auth() {
    let provider = create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        Some(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
            "openai-api-key",
        ))),
    );

    assert_eq!(provider.approval_review_preferred_model(), "gpt-5.6-luna");
}

#[test]
fn configured_provider_uses_default_approval_review_model_with_chatgpt_auth() {
    let provider = create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        Some(AuthManager::from_auth_for_testing(
            CodexAuth::create_dummy_chatgpt_auth_for_testing(),
        )),
    );

    assert_eq!(
        provider.approval_review_preferred_model(),
        DEFAULT_APPROVAL_REVIEW_PREFERRED_MODEL
    );
}

struct RotatingAuth(AtomicUsize);

impl codex_login::ExternalAuth for RotatingAuth {
    fn resolve(&self) -> codex_login::ExternalAuthFuture<'_, CodexAuth> {
        Box::pin(async move {
            let key = if self.0.fetch_add(1, Ordering::SeqCst).is_multiple_of(2) {
                "sk-proj-fedramp-synthetic-test-key"
            } else {
                "commercial-key"
            };
            Ok(CodexAuth::from_api_key(key))
        })
    }

    fn refresh(
        &self,
        _context: codex_login::ExternalAuthRefreshContext,
    ) -> codex_login::ExternalAuthFuture<'_, CodexAuth> {
        self.resolve()
    }
}

#[tokio::test]
async fn configured_provider_resolves_default_and_explicit_endpoints() {
    let provider = create_model_provider(
        provider_for("https://example.test/v1".to_string()),
        /*auth_manager*/ None,
    );

    assert_eq!(
        provider
            .runtime_base_url()
            .await
            .expect("runtime base URL should resolve"),
        Some("https://example.test/v1".to_string())
    );
    assert_eq!(
        provider
            .request_provider_and_auth(/*scope*/ None)
            .await
            .unwrap()
            .0
            .base_url,
        "https://example.test/v1"
    );

    let home = tempfile::tempdir().unwrap();
    let auth = AuthManager::from_auth_for_testing_with_home(
        CodexAuth::from_api_key("initial-key"),
        home.path().to_path_buf(),
    );
    let provider = create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        Some(auth.clone()),
    );
    for (api_key, expected_base_url) in [
        ("legacy-key", "https://api.openai.com/v1"),
        (
            "sk-proj-fedramp-synthetic-test-key",
            "https://gov.api.openai.com/v1",
        ),
        ("commercial-key", "https://api.openai.com/v1"),
    ] {
        codex_login::login_with_api_key(
            home.path(),
            api_key,
            codex_login::AuthCredentialsStoreMode::File,
            codex_login::AuthKeyringBackendKind::default(),
        )
        .unwrap();
        auth.reload().await;
        let (api_provider, resolved) = provider
            .request_provider_and_auth(/*scope*/ None)
            .await
            .unwrap();
        assert_eq!(api_provider.base_url, expected_base_url);
        let headers = resolved.auth.to_auth_headers().unwrap();
        assert_eq!(
            headers[http::header::AUTHORIZATION].to_str().unwrap(),
            format!("Bearer {api_key}")
        );
    }
    assert_eq!(provider.info().base_url, None);

    let rotating_auth = Arc::new(RotatingAuth(AtomicUsize::new(0)));
    auth.set_external_auth(rotating_auth.clone()).await.unwrap();
    for (api_key, expected_base_url, expected_fedramp_header) in [
        ("commercial-key", "https://api.openai.com/v1", None),
        (
            "sk-proj-fedramp-synthetic-test-key",
            "https://gov.api.openai.com/v1",
            Some("true"),
        ),
        ("commercial-key", "https://api.openai.com/v1", None),
    ] {
        let reads_before = rotating_auth.0.load(Ordering::SeqCst);
        let (api_provider, resolved) = provider
            .request_provider_and_auth(/*scope*/ None)
            .await
            .unwrap();
        assert_eq!(rotating_auth.0.load(Ordering::SeqCst), reads_before + 1);
        let headers = resolved.auth.to_auth_headers().unwrap();
        let expected_authorization = format!("Bearer {api_key}");
        assert_eq!(
            (
                api_provider.base_url.as_str(),
                headers
                    .get(http::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok()),
                headers
                    .get("X-OpenAI-Fedramp")
                    .and_then(|value| value.to_str().ok()),
            ),
            (
                expected_base_url,
                Some(expected_authorization.as_str()),
                expected_fedramp_header,
            )
        );
    }
}

#[test]
fn create_model_provider_builds_command_auth_manager_without_base_manager() {
    let provider = create_model_provider(
        provider_info_with_command_auth(),
        /*auth_manager*/ None,
    );

    let auth_manager = provider
        .auth_manager()
        .expect("command auth provider should have an auth manager");

    assert!(auth_manager.has_external_auth());
}

#[test]
fn create_model_provider_does_not_use_openai_auth_manager_for_amazon_bedrock_provider() {
    let provider = create_model_provider(
        ModelProviderInfo::create_amazon_bedrock_provider(Some(ModelProviderAwsAuthInfo {
            profile: Some("codex-bedrock".to_string()),
            region: None,
            credential_export: None,
            auth_refresh: None,
        })),
        Some(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
            "openai-api-key",
        ))),
    );

    assert!(provider.auth_manager().is_none());
}

#[tokio::test]
async fn create_model_provider_does_not_use_managed_auth_for_unsupported_amazon_bedrock_provider() {
    let auth = bedrock_api_key_auth();
    let provider = create_model_provider(
        ModelProviderInfo::create_amazon_bedrock_provider(/*aws*/ None),
        Some(AuthManager::from_auth_for_testing(auth.clone())),
    );

    assert_eq!(provider.auth().await, None);
}

#[test]
fn openai_provider_returns_unauthenticated_openai_account_state() {
    let provider = create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        /*auth_manager*/ None,
    );

    assert_eq!(
        provider.account_state(),
        Ok(ProviderAccountState {
            account: None,
            requires_openai_auth: true,
        })
    );
}

#[test]
fn openai_provider_returns_api_key_account_state() {
    let provider = create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        Some(AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
            "openai-api-key",
        ))),
    );

    assert_eq!(
        provider.account_state(),
        Ok(ProviderAccountState {
            account: Some(ProviderAccount::ApiKey),
            requires_openai_auth: true,
        })
    );
}

#[test]
fn openai_provider_returns_chatgpt_account_state_without_email() {
    let provider = create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        Some(AuthManager::from_auth_for_testing(
            CodexAuth::create_dummy_chatgpt_auth_for_testing(),
        )),
    );

    assert_eq!(
        provider.account_state(),
        Ok(ProviderAccountState {
            account: Some(ProviderAccount::Chatgpt {
                email: None,
                plan_type: PlanType::Unknown,
            }),
            requires_openai_auth: true,
        })
    );
}

#[test]
fn openai_provider_rejects_bedrock_api_key_account_state() {
    let provider = create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        Some(AuthManager::from_auth_for_testing(bedrock_api_key_auth())),
    );

    assert_eq!(
        provider.account_state(),
        Err(ProviderAccountError::UnsupportedBedrockApiKeyAuth)
    );
}

#[test]
fn custom_non_openai_provider_returns_no_account_state() {
    let provider = create_model_provider(
        ModelProviderInfo {
            name: "Custom".to_string(),
            base_url: Some("http://localhost:1234/v1".to_string()),
            wire_api: WireApi::Responses,
            requires_openai_auth: false,
            ..Default::default()
        },
        /*auth_manager*/ None,
    );

    assert_eq!(
        provider.account_state(),
        Ok(ProviderAccountState {
            account: None,
            requires_openai_auth: false,
        })
    );
}

#[test]
fn amazon_bedrock_provider_returns_bedrock_account_state() {
    let provider = create_model_provider(
        ModelProviderInfo::create_amazon_bedrock_provider(/*aws*/ None),
        /*auth_manager*/ None,
    );

    assert_eq!(
        provider.account_state(),
        Ok(ProviderAccountState {
            account: Some(ProviderAccount::AmazonBedrock {
                uses_codex_managed_credentials: false,
            }),
            requires_openai_auth: false,
        })
    );
}

#[tokio::test]
async fn configured_provider_models_manager_uses_provider_bearer_token() {
    let server = MockServer::start().await;
    let remote_models = vec![remote_model("provider-model")];

    Mock::given(method("GET"))
        .and(path("/models"))
        .and(header_regex("Authorization", "Bearer provider-token"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(ModelsResponse {
                    models: remote_models.clone(),
                }),
        )
        .expect(2)
        .mount(&server)
        .await;

    let mut provider_info = provider_for(server.uri());
    provider_info.experimental_bearer_token = Some("provider-token".into());
    provider_info.model_catalog_url = Some(format!("{}/models", server.uri()).into());
    provider_info.http_headers = Some(std::collections::HashMap::from([(
        codex_login::default_client::RESIDENCY_HEADER_NAME.to_string(),
        "us".into(),
    )]));
    for auth in [
        None,
        Some(CodexAuth::create_dummy_chatgpt_auth_for_testing()),
    ] {
        // Disabled discovery must ignore the catalog cached by the enabled run.
        for enabled in [true, false] {
            let provider = create_model_provider(
                provider_info.clone(),
                auth.clone().map(AuthManager::from_auth_for_testing),
            );
            let manager =
                provider.models_manager(test_codex_home(), /*config_model_catalog*/ None);
            manager.set_api_key_model_discovery_enabled(enabled);
            let refresh_strategy = if enabled {
                RefreshStrategy::Online
            } else {
                RefreshStrategy::Offline
            };
            let catalog = manager
                .raw_model_catalog(
                    refresh_strategy,
                    HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
                )
                .await;

            assert_eq!(
                catalog
                    .models
                    .iter()
                    .any(|model| model.slug == "provider-model"),
                enabled
            );
        }
    }
}
