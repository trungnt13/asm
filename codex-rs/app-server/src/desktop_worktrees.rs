//! Desktop worktree tools execute on the thread's host without moving its workspace.

use crate::outgoing_message::ThreadScopedOutgoingMessageSender;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::DynamicToolNamespaceTool;
use codex_app_server_protocol::DynamicToolSpec;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadAttachmentOperation;
use codex_app_server_protocol::ThreadAttachmentUpdatedNotification;
use codex_core::CodexThread;
use codex_core::config::ConfigBuilder;
use codex_protocol::ThreadId;
use codex_protocol::dynamic_tools::DynamicToolCallOutputContentItem;
use codex_protocol::dynamic_tools::DynamicToolResponse;
use codex_protocol::protocol::Op;
use codex_thread_store::AddThreadAttachmentOutcome;
use codex_thread_store::AddThreadAttachmentParams;
use codex_thread_store::ListThreadAttachmentsParams;
use codex_thread_store::ThreadStore;
use codex_worktree::CreateWorktree;
use codex_worktree::WorktreeManager;
use codex_worktree::WorktreeSettings;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

const KEEP_CWD: &str = "Keep the chat cwd and its skills. Use workspaceRoot explicitly for worktree commands. No workspace change or filesystem permission grant was made.";

pub(crate) fn handles(params: &DynamicToolCallParams) -> bool {
    params.namespace.as_deref() == Some("codex_app")
        && matches!(params.tool.as_str(), "create_worktree" | "attach_worktree")
}

pub(crate) fn amend_tools(tools: &mut [DynamicToolSpec]) {
    for tool in tools {
        if let DynamicToolSpec::Namespace(namespace) = tool
            && namespace.name == "codex_app"
        {
            for DynamicToolNamespaceTool::Function(function) in &mut namespace.tools {
                function.description = match function.name.as_str() {
                    "create_worktree" => format!(
                        "Create and attach a managed Git worktree on this chat's host. Follow repository instructions and prefer reusing suitable worktrees. Starts detached at ref; uncommitted and ignored files are not copied. name optionally labels the checkout directory. May return completed paths immediately. {KEEP_CWD}"
                    ),
                    "attach_worktree" => format!(
                        "Reattach a managed worktree owned by this chat. Reuses the checkout without resetting files. {KEEP_CWD}"
                    ),
                    _ => continue,
                };
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateArguments {
    allow_async: bool,
    #[serde(rename = "ref")]
    revision: String,
    name: Option<String>,
    source_root: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachArguments {
    path: PathBuf,
}

#[tracing::instrument(skip_all)]
pub(crate) async fn execute(
    params: DynamicToolCallParams,
    conversation: Arc<CodexThread>,
    store: Option<Arc<dyn ThreadStore>>,
    outgoing: ThreadScopedOutgoingMessageSender,
) {
    let result = execute_inner(&params, &conversation, store, &outgoing).await;
    let mut success = result.is_ok();
    let mut text = match result {
        Ok(value) => value.to_string(),
        Err(error) => json!({"error":error.to_string(),"message":"No client-side worktree handler was invoked; chat cwd was not changed."}).to_string(),
    };
    if text.len() > 999 {
        success = false;
        text = json!({"error":"Worktree result exceeded the output limit.","message":"Creation or registration may have completed. Inspect retained worktree attachments and checkout state before retrying; do not create a duplicate. No client-side handler was invoked. Chat cwd and skills were not changed."}).to_string();
    }
    if let Err(error) = conversation
        .submit(Op::DynamicToolResponse {
            id: params.call_id,
            response: DynamicToolResponse {
                content_items: vec![DynamicToolCallOutputContentItem::InputText { text }],
                success,
            },
        })
        .await
    {
        tracing::error!(%error, "cannot return desktop worktree tool result");
    }
}

#[tracing::instrument(skip_all)]
async fn execute_inner(
    params: &DynamicToolCallParams,
    conversation: &CodexThread,
    store: Option<Arc<dyn ThreadStore>>,
    outgoing: &ThreadScopedOutgoingMessageSender,
) -> Result<Value> {
    ensure!(
        params.arguments.to_string().len() <= 4096,
        "Worktree arguments are too large"
    );
    let store = store.context("Worktree attachment storage is unavailable")?;
    ensure!(
        store.supports_thread_attachments(),
        "Worktree attachment storage is unavailable"
    );
    let snapshot = conversation.config_snapshot().await;
    ensure!(!snapshot.ephemeral, "Ephemeral chats cannot own worktrees");
    ensure!(
        !snapshot.environment_selections().is_empty()
            && snapshot
                .environment_selections()
                .iter()
                .all(|environment| environment.environment_id
                    == codex_exec_server::LOCAL_ENVIRONMENT_ID),
        "Worktree tools require a local execution environment on this app-server host"
    );
    let config = conversation.config().await;
    ensure!(
        !config.active_project.is_untrusted(),
        "Worktree tools require a trusted source project"
    );
    let host = codex_core::config::load_config_toml_with_layer_stack(
        &config.codex_home,
        /*cwd*/ None,
        Vec::new(),
        codex_config::ConfigLoadOptions::default(),
    )
    .await?;
    let manager = WorktreeManager::new(WorktreeSettings::for_cli(
        &config.codex_home,
        host.config_toml.desktop.as_ref(),
    )?);
    let owner: ThreadId = snapshot
        .parent_thread_id
        .unwrap_or(ThreadId::from_string(&params.thread_id)?);
    let current_cwd = snapshot.cwd().to_path_buf();
    let checkout = match params.tool.as_str() {
        "create_worktree" => {
            let args: CreateArguments = serde_json::from_value(params.arguments.clone())?;
            ensure!(args.allow_async, "Use allowAsync: true");
            ensure!(
                !args.revision.is_empty() && args.revision.len() <= 128,
                "ref must contain 1 to 128 bytes"
            );
            let source_cwd = args.source_root.unwrap_or(current_cwd);
            ensure!(
                source_cwd.is_absolute(),
                "sourceRoot must be an absolute path"
            );
            let source = ConfigBuilder::default()
                .codex_home(config.codex_home.to_path_buf())
                .fallback_cwd(Some(source_cwd.clone()))
                .build()
                .await?;
            ensure!(
                !source.active_project.is_untrusted(),
                "Source project is explicitly untrusted"
            );
            let owner = owner.to_string();
            tokio::task::spawn_blocking(move || {
                let checkout = manager.create_named(
                    &CreateWorktree {
                        source_cwd,
                        base: Some(args.revision),
                    },
                    args.name.as_deref(),
                )?;
                let registration = manager.bind_thread(&checkout.root, &owner);
                Ok::<_, anyhow::Error>((checkout, registration))
            })
            .await??
        }
        "attach_worktree" => {
            let args: AttachArguments = serde_json::from_value(params.arguments.clone())?;
            ensure!(args.path.is_absolute(), "path must be absolute");
            let owner_text = owner.to_string();
            let checkout = tokio::task::spawn_blocking(move || {
                let path = std::fs::canonicalize(args.path)?;
                manager
                    .list(&path)?
                    .into_iter()
                    .find(|checkout| {
                        std::fs::canonicalize(&checkout.root)
                            .is_ok_and(|candidate| candidate == path)
                            || std::fs::canonicalize(&checkout.cwd)
                                .is_ok_and(|candidate| candidate == path)
                    })
                    .context("Path is not a managed worktree")
            })
            .await??;
            let mut cursor = None;
            let mut previously_attached = false;
            loop {
                let page = store
                    .list_thread_attachments(ListThreadAttachmentsParams {
                        thread_id: owner,
                        cursor,
                        limit: 100,
                    })
                    .await?;
                if page.attachments.iter().any(|attachment| {
                    attachment.attachment_type == "worktree"
                        && attachment.identity_key == checkout.root.to_string_lossy()
                }) {
                    previously_attached = true;
                    break;
                }
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
            let manager = WorktreeManager::new(WorktreeSettings::for_cli(
                &config.codex_home,
                host.config_toml.desktop.as_ref(),
            )?);
            tokio::task::spawn_blocking(move || {
                let existing_owner = manager.owner(&checkout.root)?;
                ensure!(
                    existing_owner.as_deref() == Some(&owner_text)
                        || (existing_owner.is_none() && previously_attached),
                    "Worktree is not owned by this chat"
                );
                let registration = manager.bind_thread(&checkout.root, &owner_text);
                Ok::<_, anyhow::Error>((checkout, registration))
            })
            .await??
        }
        _ => anyhow::bail!("Unsupported desktop worktree tool"),
    };
    let (checkout, binding) = checkout;
    let mut payload =
        json!({"root":checkout.root,"workspaceRoot":checkout.cwd,"sourceCwd":checkout.source_cwd});
    let result = store
        .add_thread_attachment(AddThreadAttachmentParams {
            thread_id: owner,
            attachment_type: "worktree".to_owned(),
            identity_key: checkout.root.to_string_lossy().into_owned(),
            payload: payload.clone(),
        })
        .await;
    match result {
        Ok(AddThreadAttachmentOutcome::Created(attachment)) => {
            outgoing
                .send_global_server_notification(ServerNotification::ThreadAttachmentUpdated(
                    ThreadAttachmentUpdatedNotification {
                        thread_id: owner.to_string(),
                        attachment_type: attachment.attachment_type,
                        identity_key: attachment.identity_key,
                        attachment_id: attachment.id,
                        operation: ThreadAttachmentOperation::Created,
                    },
                ))
                .await;
        }
        Ok(AddThreadAttachmentOutcome::Existing(attachment)) => {
            payload = attachment.payload;
        }
        Err(error) => {
            let recovery = if binding.is_ok() {
                "Use attach_worktree with its path, not create_worktree."
            } else {
                "Ownership registration also failed. Manual recovery is required; do not create another checkout."
            };
            return Ok(
                json!({"type":"registrationFailed","worktree":payload,"message":format!("Checkout retained; attachment registration failed: {error}. {recovery} {KEEP_CWD}")}),
            );
        }
    }
    if let Err(error) = binding {
        return Ok(
            json!({"type":"registrationFailed","worktree":payload,"message":format!("Checkout attached but ownership registration failed: {error}. Retry attach_worktree with its path, not create_worktree. {KEEP_CWD}")}),
        );
    }
    Ok(json!({"type":"created","worktree":payload,"workspaceRoot":checkout.cwd,"message":KEEP_CWD}))
}
