use std::sync::Arc;
use std::sync::Mutex;

use codex_extension_api::ExternalAgentEvent;
use codex_extension_api::ExternalAgentInput;
use codex_extension_api::ExternalAgentInputKind;
use codex_extension_api::ExternalAgentRuntime;
use futures::future::BoxFuture;
use serde_json::Value;
use serde_json::json;
use tokio::sync::Semaphore;

use crate::events::Observations;
use crate::transport::Commands;
use crate::transport::Exit;
use crate::transport::wait_for_exit;

pub(super) struct ClaudeRuntime {
    pub(super) commands: Arc<Commands>,
    pub(super) exit: Exit,
    pub(super) observations: Arc<Observations>,
    pub(super) submissions: Arc<Semaphore>,
    pub(super) admission: Admission,
}

#[derive(Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Running,
    Stopping,
    Closed,
}

#[derive(Default)]
struct AdmissionState {
    generation: u64,
    phase: Phase,
}

#[derive(Default)]
pub(super) struct Admission(Mutex<AdmissionState>);

impl Admission {
    fn generation(&self) -> Result<u64, String> {
        let state = self
            .0
            .lock()
            .map_err(|_| "Claude admission state poisoned")?;
        if state.phase != Phase::Running {
            return Err("Claude worker is stopping or closed; submission rejected".into());
        }
        Ok(state.generation)
    }

    fn admit(&self, generation: u64) -> Result<(), String> {
        if self.generation()? != generation {
            return Err("Claude submission invalidated by interruption".into());
        }
        Ok(())
    }

    fn stop(&self) -> Result<u64, String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "Claude admission state poisoned")?;
        if state.phase == Phase::Closed {
            return Err("Claude worker already closed".into());
        }
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or("Claude admission generation exhausted")?;
        state.phase = Phase::Stopping;
        Ok(state.generation)
    }

    fn verify_stop(&self, generation: u64) -> Result<(), String> {
        let state = self
            .0
            .lock()
            .map_err(|_| "Claude admission state poisoned")?;
        if state.phase != Phase::Stopping || state.generation != generation {
            return Err("Claude stop superseded; settlement not confirmed by this request".into());
        }
        Ok(())
    }

    fn finish_stop(&self, generation: u64, phase: Phase) -> Result<(), String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "Claude admission state poisoned")?;
        if state.phase != Phase::Stopping || state.generation != generation {
            return Err("Claude stop superseded; admission remains fenced".into());
        }
        state.phase = phase;
        Ok(())
    }
}

impl ExternalAgentRuntime for ClaudeRuntime {
    fn submit(&self, input: ExternalAgentInput) -> BoxFuture<'_, Result<(), String>> {
        // Capture before the first poll: an old, unpolled submission must not
        // become new work after an interrupt has settled its generation.
        let generation = self.admission.generation();
        let commands = Arc::clone(&self.commands);
        let observations = Arc::clone(&self.observations);
        let submissions = Arc::clone(&self.submissions);
        Box::pin(async move {
            let generation = generation?;
            let permit = submissions
                .acquire_owned()
                .await
                .map_err(|_| "Claude submission service closed".to_owned())?;
            self.admission.admit(generation)?;
            let task_id = match input.kind {
                ExternalAgentInputKind::Task => {
                    observations.begin_task()?;
                    Some(input.id.clone())
                }
                ExternalAgentInputKind::Message => None,
            };
            // Acceptance must resolve even if the caller's task is cancelled.
            // Acquire before spawning so a stop cannot overtake an admitted
            // owner that the scheduler has not polled yet.
            tokio::spawn(async move {
                let _permit = permit;
                let result = commands.request("submit", json!(input)).await;
                if let Some(id) = task_id {
                    observations.finish_task(id, &result);
                }
                result
            })
            .await
            .map_err(|error| {
                format!("Claude submission service failed; outcome unknown: {error}")
            })?
        })
    }

    fn next_event(&self) -> BoxFuture<'_, Result<Option<ExternalAgentEvent>, String>> {
        Box::pin(self.observations.next())
    }

    fn interrupt<'a>(&'a self, turn_id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let generation = self.admission.stop()?;
            let _permit = self
                .submissions
                .acquire()
                .await
                .map_err(|_| "Claude submission service closed".to_owned())?;
            self.admission.verify_stop(generation)?;
            self.commands
                .request("interrupt", json!({"turn_id": turn_id}))
                .await?;
            self.observations.reset_after_interrupt()?;
            self.admission.finish_stop(generation, Phase::Running)
        })
    }

    fn shutdown(&self) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            let generation = self.admission.stop()?;
            let _permit = self
                .submissions
                .acquire()
                .await
                .map_err(|_| "Claude submission service closed".to_owned())?;
            self.admission.verify_stop(generation)?;
            self.commands.request("shutdown", Value::Null).await?;
            let status = wait_for_exit(self.exit.clone()).await?;
            if !status.success() {
                return Err(format!("Claude bridge cleanup failed: {status}"));
            }
            self.admission.finish_stop(generation, Phase::Closed)
        })
    }
}
