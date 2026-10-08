use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::MutexGuard;

use codex_extension_api::ExternalAgentEvent;
use codex_extension_api::ExternalObservation;
use tokio::sync::Notify;

enum Failure {
    Receipt(String),
    Transport(String),
}

#[derive(Default)]
struct State {
    queue: VecDeque<ExternalAgentEvent>,
    failure: Option<Failure>,
    closed: bool,
    latest_task: String,
    pending_task: bool,
}

#[derive(Default)]
pub(super) struct Observations {
    state: Mutex<State>,
    changed: Notify,
}

impl Observations {
    fn state(&self) -> Result<MutexGuard<'_, State>, String> {
        self.state
            .lock()
            .map_err(|_| "Claude observation state poisoned".into())
    }

    pub(super) fn push(&self, event: ExternalAgentEvent) -> Result<(), String> {
        let mut state = self.state()?;
        if state.queue.len() >= 64 {
            return Err(
                "Claude observation queue overflowed; shutdown requested, settlement unknown"
                    .into(),
            );
        }
        state.queue.push_back(event);
        self.changed.notify_one();
        Ok(())
    }

    pub(super) fn fail(&self, error: String) {
        if let Ok(mut state) = self.state() {
            state.failure = Some(Failure::Transport(error));
        }
        self.changed.notify_waiters();
    }

    pub(super) fn finish_stream(&self, failure: Option<String>) {
        if let Ok(mut state) = self.state() {
            if let Some(failure) = failure {
                state.failure = Some(Failure::Transport(failure));
            }
            state.closed = true;
        }
        self.changed.notify_waiters();
    }

    pub(super) fn begin_task(&self) -> Result<(), String> {
        self.state()?.pending_task = true;
        Ok(())
    }

    pub(super) fn finish_task(&self, id: String, result: &Result<(), String>) {
        if let Ok(mut state) = self.state() {
            state.pending_task = false;
            match result {
                Ok(()) => state.latest_task = id,
                Err(error) => {
                    if !matches!(state.failure, Some(Failure::Transport(_))) {
                        state.failure = Some(Failure::Receipt(format!(
                            "Claude task receipt failed: {error}; outcome unknown"
                        )));
                    }
                }
            }
        }
        self.changed.notify_waiters();
    }

    pub(super) fn reset_after_interrupt(&self) -> Result<(), String> {
        let mut state = self.state()?;
        if state.closed || matches!(state.failure, Some(Failure::Transport(_))) {
            return Err(
                "Claude execution settled, but transport is unusable; shutdown and reopen required"
                    .into(),
            );
        }
        state.queue.clear();
        state.failure = None;
        state.latest_task.clear();
        state.pending_task = false;
        self.changed.notify_waiters();
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    pub(super) async fn next(&self) -> Result<Option<ExternalAgentEvent>, String> {
        loop {
            // Register before inspecting state so wakeups cannot be missed. Never
            // remove an observation unless this future can return without awaiting.
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            {
                let mut state = self.state()?;
                if let Some(Failure::Receipt(error) | Failure::Transport(error)) = &state.failure {
                    return Err(error.clone());
                }
                while let Some(event) = state.queue.front() {
                    if matches!(event.kind, ExternalObservation::ReadyToFinish { .. }) {
                        if state.pending_task {
                            break;
                        }
                        if event.id != format!("finish:{}", state.latest_task) {
                            state.queue.pop_front();
                            continue;
                        }
                    }
                    let event = state
                        .queue
                        .pop_front()
                        .ok_or("Claude observation queue inconsistent")?;
                    if event.turn_id.is_empty()
                        && let ExternalObservation::Failed(error) = event.kind
                    {
                        return Err(error);
                    }
                    return Ok(Some(event));
                }
                if state.closed && state.queue.is_empty() {
                    return Ok(None);
                }
            }
            changed.await;
        }
    }
}
