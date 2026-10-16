use super::*;
use codex_app_server_protocol::ThreadSystem1StatsReadParams;
use codex_app_server_protocol::ThreadSystem1StatsReadResponse;

impl ThreadRequestProcessor {
    #[tracing::instrument(skip_all)]
    pub(crate) async fn thread_system1_stats_read(
        &self,
        params: ThreadSystem1StatsReadParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        // load_thread only looks up resident runtimes; it never resumes stored threads.
        let (_, thread) = self.load_thread(&params.thread_id).await?;
        let snapshot = thread.config_snapshot().await;
        Ok(Some(
            ThreadSystem1StatsReadResponse {
                enabled: snapshot.system1_enabled,
                revision: snapshot.system1_revision,
                error: snapshot.system1_error,
                failure_episode: snapshot.system1_failure_episode,
                reasoning_effort: snapshot.reasoning_effort,
                stats: thread.system1_stats(),
            }
            .into(),
        ))
    }
}
