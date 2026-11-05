//! Applies the selected server's socket policy on initial connections and reconnects.

use crate::AppServerTarget;
#[cfg(windows)]
use crate::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY;
use crate::connect_remote_app_server;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerRequestHandle;
#[cfg(windows)]
use codex_app_server_client::RemoteAppServerClient;
#[cfg(windows)]
use codex_app_server_client::RemoteAppServerConnectArgs;
#[cfg(windows)]
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::AsmTranscriptEnableParams;
use codex_app_server_protocol::AsmTranscriptEnableResponse;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
#[cfg(windows)]
use codex_utils_absolute_path::AbsolutePathBuf;

pub(crate) async fn connect(target: &AppServerTarget) -> color_eyre::Result<AppServerClient> {
    let client = match target {
        AppServerTarget::Embedded => {
            color_eyre::eyre::bail!("embedded sessions have no remote connection")
        }
        #[cfg(windows)]
        AppServerTarget::LocalDaemon {
            endpoint: RemoteAppServerEndpoint::UnixSocket { socket_path },
            ..
        } => {
            // Revalidate at the real connection, not just the earlier discovery probe.
            let (socket_path, _directory) =
                codex_uds::validate_private_socket_path(socket_path.as_path())?;
            let app_server =
                RemoteAppServerClient::connect_local_daemon(RemoteAppServerConnectArgs {
                    required_human_approval: true,
                    endpoint: RemoteAppServerEndpoint::UnixSocket {
                        socket_path: AbsolutePathBuf::from_absolute_path_checked(socket_path)?,
                    },
                    client_name: "codex-tui".to_string(),
                    client_version: env!("CARGO_PKG_VERSION").to_string(),
                    experimental_api: true,
                    mcp_server_openai_form_elicitation: false,
                    opt_out_notification_methods: Vec::new(),
                    channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
                })
                .await?;
            Ok(AppServerClient::Remote(app_server))
        }
        AppServerTarget::LocalDaemon { endpoint, .. } | AppServerTarget::Remote { endpoint } => {
            connect_remote_app_server(endpoint.clone()).await
        }
    }?;
    enable_asm_transcript(client.request_handle()).await?;
    Ok(client)
}

#[tracing::instrument(skip_all)]
pub(crate) async fn enable_asm_transcript(
    handle: AppServerRequestHandle,
) -> color_eyre::Result<()> {
    let response = handle
        .request_typed::<AsmTranscriptEnableResponse>(ClientRequest::AsmTranscriptEnable {
            request_id: RequestId::String(uuid::Uuid::new_v4().to_string()),
            params: AsmTranscriptEnableParams { version: 1 },
        })
        .await;
    match response {
        Ok(response) if response.version == 1 => Ok(()),
        Ok(response) => {
            color_eyre::eyre::bail!("unsupported ASM transcript version: {}", response.version)
        }
        Err(TypedRequestError::Server { source, .. })
            if source.code == -32601
                || (source.code == -32600
                    && source.message.starts_with(
                        "Invalid request: unknown variant `asmTranscript/enable`, expected ",
                    )) =>
        {
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}
