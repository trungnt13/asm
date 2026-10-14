use super::Session;
use super::thread_settings;
use crate::context::CompanionLifecycle;
use crate::context::ContextualUserFragment;
use crate::context::validate_injected_item_size;
use codex_history::ResponseItemEnvelope;
use codex_history::RolloutItem;
use codex_protocol::companion::CompanionLifecycleEvent;
use codex_thread_store::PersistContext;

impl Session {
    #[tracing::instrument(level = "trace", skip_all)]
    pub(crate) async fn record_companion_lifecycle(
        &self,
        event: CompanionLifecycleEvent,
    ) -> std::io::Result<()> {
        // Context construction must not hold the persistence permit or initialize a step.
        let context = Box::pin(self.new_inject_items_context()).await;
        let mut item = ContextualUserFragment::into(CompanionLifecycle(event));
        Self::stamp_response_item_for_history(&mut item, &context.sub_id);
        Self::assign_missing_response_item_id(&mut item);
        validate_injected_item_size(&item)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
        let _guard = thread_settings::acquire_persistence_lock(self).await;
        let mut item = ResponseItemEnvelope::new(item);
        if let Some(live_thread) = self.live_thread() {
            live_thread
                .append_items(&[
                    RolloutItem::EventMsg(thread_settings::applied_event(self).await),
                    RolloutItem::ResponseItem(item.clone()),
                ])
                .await
                .map_err(std::io::Error::other)?;
        }
        // Publish only after the durability barrier, without pending input or turn admission.
        self.try_ensure_rollout_materialized(PersistContext::ThreadPreparation)
            .await?;
        self.state.lock().await.history.record_annotated_items(
            std::slice::from_mut(&mut item),
            context.model_info().truncation_policy.into(),
        );
        self.send_raw_response_items(&context, std::slice::from_ref(&item.item))
            .await;
        Ok(())
    }
}
