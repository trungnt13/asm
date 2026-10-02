use std::sync::Arc;

use anyhow::Result;
use codex_analytics::AnalyticsEventsClient;
use codex_core::PromptRequestAudit;
use codex_core::StartThreadOptions;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_exec_server::EnvironmentManager;
use codex_exec_server::ExecServerRuntimeOptions;
use codex_extension_api::ExtensionEventSink;
use codex_extension_api::ExtensionWarning;
use codex_goal_extension::GoalService;
use codex_home::CodexHomeUserInstructionsProvider;
use codex_login::AuthManager;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SessionSource;
use codex_protocol::user_input::UserInput;
use futures::FutureExt;
use serde_json::Value;

use crate::extensions::ThreadExtensionDependencies;
use crate::extensions::thread_extensions;
use crate::turn_admission::TurnAdmission;

/// Captures a fresh request with the app-server's configured extensions.
/// Startup may contact configured services or update local caches.
/// Non-ephemeral configuration also initializes persistent thread state.
/// No inference request is sent.
#[doc(hidden)]
#[tracing::instrument(skip_all)]
pub async fn build_prompt_request(
    config: Config,
    input: Vec<UserInput>,
    output_schema: Option<Value>,
) -> Result<PromptRequestAudit> {
    let auth_manager =
        AuthManager::shared_from_config(&config, /*enable_codex_api_key_env*/ false).await?;
    codex_login::GatewayLoginControl::for_runtime(&auth_manager.runtime_config())
        .require_explicit_login();
    let local_runtime_paths = ExecServerRuntimeOptions::from_optional_paths(
        config.codex_self_exe.clone(),
        config.codex_linux_sandbox_exe.clone(),
    )?;
    let environment_manager = Arc::new(
        EnvironmentManager::from_codex_home(
            config.codex_home.clone(),
            Some(local_runtime_paths),
            config.http_client_factory(),
        )
        .await?,
    );
    // Goal capabilities depend on a real persistent thread, not just their feature flag.
    // Preserve configured persistence; setup audits supply their own temporary home.
    let state_db = if config.ephemeral {
        None
    } else {
        Some(codex_rollout::state_db::try_init(&config).await?)
    };
    let thread_store = codex_core::thread_store_from_config(&config, state_db.clone());
    let installation_id = codex_core::resolve_installation_id(&config.codex_home).await?;
    let executor_skill_provider = Arc::new(
        codex_skills_extension::ExecutorSkillProvider::new_with_restriction_product(
            Arc::clone(&environment_manager),
            SessionSource::Exec.restriction_product(),
        ),
    );
    let event_sink: Arc<dyn ExtensionEventSink> = Arc::new(PromptAuditEventSink);
    let thread_manager = Arc::new_cyclic(|thread_manager| {
        ThreadManager::new(
            &config,
            auth_manager.clone(),
            codex_core::build_models_manager(&config, auth_manager.clone()),
            codex_core::CodexAppsToolsCache::default(),
            SessionSource::Exec,
            Arc::clone(&environment_manager),
            thread_extensions(ThreadExtensionDependencies {
                event_sink: Arc::clone(&event_sink),
                auth_manager: auth_manager.clone(),
                state_db: state_db.clone(),
                analytics_events_client: AnalyticsEventsClient::disabled(),
                thread_manager: thread_manager.clone(),
                goal_service: Arc::new(GoalService::new()),
                environment_manager: Arc::clone(&environment_manager),
                executor_skill_provider: executor_skill_provider.clone(),
                git_attribution_base_url: config.chatgpt_base_url.clone(),
                http_client_factory: config.http_client_factory(),
                // Queue installation starts a background dispatch watcher, not prompt tools.
                // A standalone audit must not submit queued user messages for inference.
                queue_service: None,
                turn_start_admission: Some(Arc::new(TurnAdmission::default())),
            }),
            Arc::new(CodexHomeUserInstructionsProvider::new(
                config.codex_home.clone(),
            )),
            /*analytics_events_client*/ None,
            codex_core::passthrough_image_store(),
            thread_store,
            codex_core::local_agent_graph_store_from_state_db(state_db.as_ref()),
            installation_id,
            /*attestation_provider*/ None,
            /*external_time_provider*/ None,
        )
    });
    let thread = thread_manager
        .start_thread(StartThreadOptions::new(config))
        .await?;
    let output =
        codex_core::build_prompt_request_from_thread(&thread.thread, input, output_schema).await;
    let shutdown = thread.thread.shutdown_and_wait().await;
    // Core warnings use the thread event queue, not the extension event sink.
    // Shutdown has finished; drain buffered events without waiting on an empty queue.
    while let Some(Ok(event)) = thread.thread.next_event().now_or_never() {
        let shutdown_complete = matches!(event.msg, EventMsg::ShutdownComplete);
        event_sink.emit(event);
        if shutdown_complete {
            break;
        }
    }
    let _removed = thread_manager.remove_thread(&thread.thread_id).await;
    shutdown?;
    Ok(output?)
}

struct PromptAuditEventSink;

// Keep audit diagnostics visible on stderr while stdout remains strict JSON.
#[allow(clippy::print_stderr)]
impl ExtensionEventSink for PromptAuditEventSink {
    fn emit(&self, event: Event) {
        match &event.msg {
            EventMsg::Warning(warning) => {
                eprintln!("Prompt audit warning: {}", warning.message);
            }
            EventMsg::DeprecationNotice(notice) => {
                eprintln!("Prompt audit deprecation: {}", notice.summary);
                if let Some(details) = &notice.details {
                    eprintln!("{details}");
                }
            }
            _ => tracing::debug!(?event, "prompt audit extension event"),
        }
    }

    fn emit_warning(&self, warning: ExtensionWarning) {
        eprintln!("Prompt audit warning: {}", warning.message);
    }
}
