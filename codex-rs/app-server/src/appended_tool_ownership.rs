//! Connection-local execution ownership for append-only dynamic tools.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

use codex_core::CodexThread;
use codex_protocol::ThreadId;
use codex_protocol::dynamic_tools::DynamicToolSpec;

use crate::error_code::invalid_request;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::OutgoingMessageSender;
use crate::thread_state::ThreadStateManager;
use codex_app_server_protocol::JSONRPCErrorError;

#[derive(Default)]
pub(super) struct AppendedToolOwners {
    pub(super) live_connections: HashSet<ConnectionId>,
    // Keep ownerless declarations after disconnect: absence would restore broadcast routing.
    pub(super) tools: HashMap<(ThreadId, String), Option<ConnectionId>>,
    admissions: HashMap<ThreadId, Arc<Mutex<()>>>,
}

impl AppendedToolOwners {
    fn revoke_tools(&mut self, matches: impl Fn(ThreadId, Option<ConnectionId>) -> bool) {
        for ((thread_id, _), owner) in &mut self.tools {
            if matches(*thread_id, *owner) {
                *owner = None;
            }
        }
    }
}

impl OutgoingMessageSender {
    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn appended_tool_connection_initialized(&self, connection_id: ConnectionId) {
        self.appended_tool_owners
            .lock()
            .await
            .live_connections
            .insert(connection_id);
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn append_connection_tools(
        &self,
        thread_id: ThreadId,
        connection_id: ConnectionId,
        thread: &CodexThread,
        thread_state_manager: &ThreadStateManager,
        tools: Vec<DynamicToolSpec>,
    ) -> Result<(), JSONRPCErrorError> {
        let admission = self
            .appended_tool_owners
            .lock()
            .await
            .admissions
            .entry(thread_id)
            .or_default()
            .clone();
        let _admission = admission.lock().await;
        let owners = self.appended_tool_owners.lock().await;
        if !owners.live_connections.contains(&connection_id) {
            return Err(invalid_request("tool registration connection is closed"));
        }
        let names = tools
            .iter()
            .map(|spec| match spec {
                DynamicToolSpec::Function(tool) => Ok(tool.name.clone()),
                DynamicToolSpec::Namespace(_) => Err(invalid_request(
                    "appended tools must be standalone functions",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        for name in &names {
            if owners
                .tools
                .get(&(thread_id, name.clone()))
                .copied()
                .flatten()
                .is_some_and(|owner| owner != connection_id)
            {
                return Err(invalid_request(format!(
                    "appended tool is owned by another connection: {name}"
                )));
            }
        }
        // Core preparation may invoke client callbacks; never hold the routing lock here.
        drop(owners);
        thread
            .append_tools(tools)
            .await
            .map_err(|error| invalid_request(error.to_string()))?;
        let mut owners = self.appended_tool_owners.lock().await;
        if !owners.live_connections.contains(&connection_id) {
            return Err(invalid_request("tool registration connection is closed"));
        }
        // Unsubscribe removes membership before taking this routing lock to revoke ownership.
        // Recheck under the lock so a concurrent unsubscribe cannot leave a renewed owner.
        if !thread_state_manager
            .subscribed_connection_ids(thread_id)
            .await
            .contains(&connection_id)
        {
            return Err(invalid_request(
                "tool registration requires a thread subscription",
            ));
        }
        for name in names {
            owners.tools.insert((thread_id, name), Some(connection_id));
        }
        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn clear_appended_tool_owners(&self, thread_id: ThreadId) {
        let admission = self
            .appended_tool_owners
            .lock()
            .await
            .admissions
            .get(&thread_id)
            .cloned();
        let _admission = match &admission {
            Some(admission) => Some(admission.lock().await),
            None => None,
        };
        let mut owners = self.appended_tool_owners.lock().await;
        owners.revoke_tools(|registered_thread, _| registered_thread == thread_id);
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn disconnect_appended_tool_connection(&self, connection_id: ConnectionId) {
        let mut owners = self.appended_tool_owners.lock().await;
        owners.live_connections.remove(&connection_id);
        owners.revoke_tools(|_, owner| owner == Some(connection_id));
        self.request_id_to_callback
            .lock()
            .await
            .retain(|_, entry| entry.connection_owner != Some(connection_id));
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn release_appended_tool_connection(
        &self,
        connection_id: ConnectionId,
        thread_id: ThreadId,
    ) {
        let mut owners = self.appended_tool_owners.lock().await;
        owners.revoke_tools(|registered_thread, owner| {
            registered_thread == thread_id && owner == Some(connection_id)
        });
        self.request_id_to_callback.lock().await.retain(|_, entry| {
            entry.thread_id != Some(thread_id)
                || entry.connection_owner != Some(connection_id)
                || entry.verification_owner.is_some()
        });
    }
}
