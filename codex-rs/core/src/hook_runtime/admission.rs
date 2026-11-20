use super::additional_context_messages;
use super::inspect_pending_input;
use super::record_additional_contexts;
use super::record_pending_input;
use crate::session::TurnInput;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelInfo;
use codex_thread_store::PersistContext;
use std::borrow::Cow;
use std::sync::Arc;
use tracing::instrument;

pub(crate) struct InputAdmission<'a> {
    input: Option<Cow<'a, TurnInput>>,
    hook_contexts: Vec<codex_hooks::HookContext>,
}

impl InputAdmission<'_> {
    pub(crate) fn input(&self) -> Option<&TurnInput> {
        self.input.as_deref()
    }

    pub(crate) fn input_mut(&mut self) -> Option<&mut TurnInput> {
        self.input.as_mut().map(Cow::to_mut)
    }

    pub(crate) fn hook_context_messages(&self) -> Vec<ResponseItem> {
        additional_context_messages(self.hook_contexts.clone())
    }

    #[instrument(level = "trace", skip_all)]
    pub(crate) async fn record(
        self,
        sess: &Arc<Session>,
        turn: &Arc<TurnContext>,
        model: &ModelInfo,
        persist_context: PersistContext,
    ) -> std::io::Result<()> {
        if let Some(input) = self.input {
            record_pending_input(
                sess,
                turn,
                model,
                input.into_owned(),
                self.hook_contexts,
                persist_context,
            )
            .await
        } else {
            record_additional_contexts(sess, turn, self.hook_contexts).await;
            Ok(())
        }
    }
}

#[instrument(level = "trace", skip_all)]
pub(crate) async fn admit_pending_input<'a>(
    sess: &Arc<Session>,
    turn: &Arc<TurnContext>,
    input: Cow<'a, TurnInput>,
) -> InputAdmission<'a> {
    let outcome = inspect_pending_input(sess, turn, &input).await;
    if outcome.should_stop
        && let TurnInput::UserInput { metadata, .. } = input.as_ref()
        && let Some(id) = metadata.realtime_handoff_id
    {
        sess.reject_realtime_handoff(id).await;
    }
    InputAdmission {
        input: (!outcome.should_stop).then_some(input),
        hook_contexts: outcome.additional_contexts,
    }
}
