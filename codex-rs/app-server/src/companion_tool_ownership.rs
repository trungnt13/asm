//! Connection ownership for the companion tool declared when a thread starts.

use std::collections::HashMap;
use std::collections::HashSet;

use codex_app_server_protocol::JSONRPCErrorError;
use codex_core::CodexThread;
use codex_protocol::ThreadId;
use codex_protocol::dynamic_tools::DynamicToolSpec;

use crate::error_code::invalid_request;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::OutgoingMessageSender;
use crate::thread_state::ThreadStateManager;

#[derive(Default)]
pub(super) struct CompanionToolOwners {
    pub(super) live_connections: HashSet<ConnectionId>,
    pub(super) threads: HashMap<ThreadId, ConnectionId>,
}

impl OutgoingMessageSender {
    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn companion_connection_initialized(&self, connection_id: ConnectionId) {
        self.companion_tool_owners
            .lock()
            .await
            .live_connections
            .insert(connection_id);
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn claim_companion_tool(
        &self,
        thread_id: ThreadId,
        thread: &CodexThread,
        connection_id: ConnectionId,
        thread_state_manager: &ThreadStateManager,
    ) -> Result<(), JSONRPCErrorError> {
        let config = thread.config().await;
        let declared = thread.dynamic_tools().await.iter().any(|spec| {
            matches!(spec, DynamicToolSpec::Function(tool) if tool.name == "send_companion_message")
        }) || config.mcp_servers.get().iter().any(|(name, server)| {
            matches!(name.as_str(), "codex_tui" | "codex_companion")
                && server.enabled
                && server.tools.contains_key("send_companion_message")
        });
        let mut owners = self.companion_tool_owners.lock().await;
        if owners
            .threads
            .get(&thread_id)
            .is_some_and(|owner| *owner != connection_id)
        {
            drop(owners);
            thread_state_manager
                .unsubscribe_connection_from_thread(thread_id, connection_id)
                .await;
            return Err(invalid_request(
                "companion communication is owned by another connection",
            ));
        }
        if !config.enable_side_communication
            || !declared
            || !owners.live_connections.contains(&connection_id)
            || !thread_state_manager
                .subscribed_connection_ids(thread_id)
                .await
                .contains(&connection_id)
        {
            return Ok(());
        }
        owners.threads.insert(thread_id, connection_id);
        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn check_companion_connection(
        &self,
        thread_id: ThreadId,
        connection_id: ConnectionId,
    ) -> Result<(), JSONRPCErrorError> {
        let owners = self.companion_tool_owners.lock().await;
        if owners
            .threads
            .get(&thread_id)
            .is_some_and(|owner| *owner != connection_id)
        {
            return Err(invalid_request(
                "companion communication is owned by another connection",
            ));
        }
        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn owns_companion_tool(
        &self,
        thread_id: ThreadId,
        connection_id: ConnectionId,
    ) -> bool {
        let owners = self.companion_tool_owners.lock().await;
        owners.threads.get(&thread_id) == Some(&connection_id)
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn clear_companion_tool_owner(&self, thread_id: ThreadId) {
        let mut owners = self.companion_tool_owners.lock().await;
        owners.threads.remove(&thread_id);
        self.request_id_to_callback.lock().await.retain(|_, entry| {
            entry.thread_id != Some(thread_id)
                || entry.connection_owner.is_none()
                || entry.verification_owner.is_some()
        });
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn disconnect_companion_connection(&self, connection_id: ConnectionId) {
        let mut owners = self.companion_tool_owners.lock().await;
        owners.live_connections.remove(&connection_id);
        owners.threads.retain(|_, owner| *owner != connection_id);
        self.request_id_to_callback.lock().await.retain(|_, entry| {
            entry.connection_owner != Some(connection_id) || entry.verification_owner.is_some()
        });
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn release_companion_connection(
        &self,
        connection_id: ConnectionId,
        thread_id: ThreadId,
    ) {
        let mut owners = self.companion_tool_owners.lock().await;
        if owners.threads.get(&thread_id) == Some(&connection_id) {
            owners.threads.remove(&thread_id);
        }
        self.request_id_to_callback.lock().await.retain(|_, entry| {
            entry.thread_id != Some(thread_id)
                || entry.connection_owner != Some(connection_id)
                || entry.verification_owner.is_some()
        });
    }
}
