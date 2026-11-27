//! Cursor has no ASM sampling boundary inside an SDK run. Deliver mail outside the
//! Code Mode cell, so even an unprinted or yielded wait result cannot hide it.

use super::Checkpoint;
use super::input;
use super::input_messages;
use super::input_prefix;
use super::materialize_native_inputs;
use crate::hook_runtime::record_pending_input;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use anyhow::Context;
use anyhow::ensure;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

pub(crate) struct CursorMailbox {
    woken: AtomicBool,
    progress: Mutex<Progress>,
}

struct Progress {
    input_count: usize,
    input_prefix: Option<String>,
    batches: u64,
}

impl CursorMailbox {
    pub(super) fn new(input_count: usize) -> Self {
        Self {
            woken: AtomicBool::new(false),
            progress: Mutex::new(Progress {
                input_count,
                input_prefix: None,
                batches: 0,
            }),
        }
    }

    pub(crate) fn notify_wait(&self) {
        self.woken.store(true, Ordering::Release);
    }

    pub(super) fn take_wake(&self) -> bool {
        self.woken.swap(false, Ordering::AcqRel)
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn append(
        &self,
        session: &Arc<Session>,
        turn: &Arc<TurnContext>,
        body: &mut Value,
    ) -> anyhow::Result<()> {
        let state = session
            .input_queue
            .turn_state_for_sub_id(&session.active_turn, &turn.sub_id)
            .await
            .context("Cursor mailbox turn has stopped")?;
        let content = body["content"]
            .as_array_mut()
            .context("Missing ASM tool result content")?;
        let start = {
            let progress = self
                .progress
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            ensure!(
                progress.batches == 0 || progress.input_prefix.is_some(),
                "Cursor mailbox delivery is uncertain"
            );
            progress.input_count
        };
        let pending = session.input_queue.take_current_turn_mail(&state).await;
        if pending.is_empty() {
            content.push(json!({"type":"text","text":"No mail is available for this callback. End the current response to receive any deferred input; do not wait again for it."}));
            return Ok(());
        }
        {
            let mut progress = self
                .progress
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            progress.batches += 1;
            progress.input_prefix = None;
        }
        let mut pending = materialize_native_inputs(session, turn, pending);
        if input::prepare_input(&mut pending)? > 0 {
            session
                .send_event(
                    turn,
                    EventMsg::Warning(WarningEvent {
                        message: "Encrypted ASM input was ignored; the user prompt is preserved."
                            .to_owned(),
                    }),
                )
                .await;
        }
        for entry in pending {
            record_pending_input(
                session,
                turn,
                &turn.initial_settings.model_info,
                entry,
                Vec::new(),
                codex_thread_store::PersistContext::TurnStart,
            )
            .await?;
        }
        session.flush_rollout().await?;
        let messages = input_messages(session.clone_history().await.raw_items())?;
        for message in messages
            .get(start..)
            .context("Cursor mailbox input history changed")?
        {
            let text = message
                .as_str()
                .context("Cursor mailbox requires text input")?;
            content.push(json!({"type":"text","text":text}));
        }
        let prefix = input_prefix(&messages)?;
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        progress.input_count = messages.len();
        progress.input_prefix = Some(prefix);
        body["mailBatch"] = json!(true);
        Ok(())
    }

    pub(super) fn confirm(
        &self,
        result: &Value,
        checkpoint: &mut Checkpoint,
    ) -> anyhow::Result<()> {
        let progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure!(
            result["receivedMailBatches"].as_u64().unwrap_or_default() == progress.batches,
            "Cursor SDK did not receive all mailbox results; start a new chat and check saved history"
        );
        if let Some(prefix) = &progress.input_prefix {
            checkpoint.input_count = progress.input_count;
            checkpoint.input_prefix.clone_from(prefix);
        }
        Ok(())
    }
}
