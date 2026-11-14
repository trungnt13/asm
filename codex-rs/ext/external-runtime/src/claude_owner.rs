use crate::ExternalTurnOutcome;
use crate::Prompt;
use crate::claude::ClaudeDriver;
use crate::claude::ClaudeOptions;
use anyhow::Context;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub struct ClaudeContinuation {
    pub generation: u64,
    pub steering: Option<crate::ClaudeSteering>,
    pub admitted: oneshot::Sender<()>,
    pub superseded: CancellationToken,
    pub cancellation: CancellationToken,
    pub events: mpsc::Receiver<Value>,
    pub outcome: oneshot::Receiver<Result<ExternalTurnOutcome>>,
}

struct TurnRequest {
    prompt: Prompt,
    events: mpsc::Sender<Value>,
    cancel: CancellationToken,
    steering: Option<mpsc::Receiver<Prompt>>,
    reply: oneshot::Sender<Result<(ExternalTurnOutcome, u64)>>,
}

enum Request {
    Turn(TurnRequest),
    Effort(Option<String>, oneshot::Sender<Result<()>>),
    Shutdown(oneshot::Sender<Result<()>>),
}

#[derive(Default)]
struct Snapshot {
    background_work: bool,
    fork_point: Option<String>,
    diagnostics: Value,
}

type CheckpointInvalidator = Box<dyn Fn() -> Result<()> + Send + Sync>;

pub struct ClaudeRuntime {
    requests: mpsc::Sender<Request>,
    continuations: Option<mpsc::Receiver<ClaudeContinuation>>,
    snapshot: watch::Receiver<Snapshot>,
    mid_turn_delivery: watch::Receiver<bool>,
    generation: Arc<AtomicU64>,
    checkpoint: Arc<Mutex<Option<CheckpointInvalidator>>>,
    projected: AtomicU64,
    completed_generation: u64,
    active_cancel: Arc<Mutex<Option<CancellationToken>>>,
    owner: JoinHandle<()>,
}

impl ClaudeRuntime {
    #[tracing::instrument(skip_all)]
    pub async fn spawn(options: ClaudeOptions) -> Result<Self> {
        let child = matches!(
            options.session_kind,
            crate::claude::ClaudeSessionKind::ManagedChild
        );
        let mut driver = ClaudeDriver::spawn(options).await?;
        let mid_turn_delivery = driver.subscribe_mid_turn_delivery();
        let (requests, mut receiver) = mpsc::channel(/*buffer*/ 1);
        let (continuation_tx, continuations) = mpsc::channel(/*buffer*/ 1);
        let (snapshot_tx, snapshot) = watch::channel(Snapshot::default());
        let generation = Arc::new(AtomicU64::new(/*v*/ 0));
        let activity = Arc::clone(&generation);
        let checkpoint = Arc::new(Mutex::new(None::<CheckpointInvalidator>));
        let publication = Arc::clone(&checkpoint);
        let active_cancel = Arc::new(Mutex::new(None));
        let cancellation = Arc::clone(&active_cancel);
        let owner = tokio::spawn(async move {
            loop {
                let request = tokio::select! {
                    biased;
                    frame = driver.process.frames.recv() => {
                        let frame = match frame {
                            Some(Ok(frame)) => frame,
                            Some(Err(_)) | None => break,
                        };
                        let generation = {
                            let publication = publication.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                            let metadata = frame["type"] == "command_lifecycle"
                                || (frame["type"] == "system" && matches!(frame["subtype"].as_str(),
                                    Some("init" | "per_turn_effort_changed" | "background_tasks_changed")))
                                || (frame["type"] == "system" && frame["subtype"] == "session_state_changed" && frame["state"] == "idle");
                            let background_start = frame["type"] == "system" && frame["subtype"] == "background_tasks_changed"
                                && frame["tasks"].as_array().is_some_and(|tasks| tasks.iter().any(|task| task["ambient"] != true));
                            if (!metadata || background_start) && let Some(invalidate) = publication.as_ref()
                                && invalidate().is_err() { break; }
                            if driver.observe_session_frame(&frame).is_err() { break; }
                            snapshot_tx.send_modify(|snapshot| snapshot.background_work = driver.background_work);
                            if metadata { continue; }
                            activity.fetch_add(/*val*/ 1, Ordering::AcqRel) + 1
                        };
                        let (steering_tx, steering_rx) = crate::ClaudeSteering::channel();
                        let (events_tx, events) = mpsc::channel(/*buffer*/ 32);
                        let (outcome_tx, outcome) = oneshot::channel();
                        let (admitted, admission) = oneshot::channel();
                        let superseded = CancellationToken::new();
                        let cancel = CancellationToken::new();
                        if continuation_tx.send(ClaudeContinuation {
                            generation, steering: child.then_some(steering_tx), admitted, superseded: superseded.clone(), cancellation: cancel.clone(), events, outcome,
                        }).await.is_err() {
                            break;
                        }
                        driver.first_frame = Some(frame);
                        // A foreground task can win admission before the notification. In that
                        // case its real task envelope owns projection, but input waits for idle.
                        let foreground = tokio::select! {
                            biased;
                            admitted = admission => {
                                if admitted.is_err() { break; }
                                None
                            },
                            request = receiver.recv() => match request {
                                Some(Request::Turn(request)) => { superseded.cancel(); Some(request) },
                                Some(Request::Effort(_, reply)) => {
                                    let _ = reply.send(Err(anyhow::anyhow!("Claude has pending native activity; wait until idle")));
                                    superseded.cancel();
                                    break;
                                },
                                Some(Request::Shutdown(reply)) => {
                                    superseded.cancel();
                                    let _ = reply.send(driver.shutdown().await);
                                    break;
                                },
                                None => break,
                            },
                        };
                        let sink = foreground.as_ref().map_or(&events_tx, |request| &request.events);
                        let cancel = foreground.as_ref().map_or(cancel, |request| request.cancel.clone());
                        *cancellation.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cancel.clone());
                        let result = driver.read_result(sink, cancel, "", child.then_some(steering_rx)).await;
                        *cancellation.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                        snapshot_tx.send_replace(Snapshot {
                            background_work: driver.has_background_work(),
                            fork_point: driver.fork_point().map(str::to_owned),
                            diagnostics: driver.diagnostic_snapshot(),
                        });
                        if let Some(request) = foreground {
                            if let Err(error) = result {
                                let _ = request.reply.send(Err(error));
                                break;
                            }
                            Request::Turn(request)
                        } else {
                            let failed = result.is_err();
                            let _ = outcome_tx.send(result);
                            if failed { break; }
                            continue;
                        }
                    }
                    request = receiver.recv() => match request {
                        Some(request) => request,
                        None => break,
                    },
                };
                match request {
                    Request::Turn(request) => {
                        *cancellation
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) =
                            Some(request.cancel.clone());
                        let result = driver
                            .run_turn(
                                &request.prompt,
                                request.events,
                                request.cancel,
                                request.steering,
                            )
                            .await;
                        *cancellation
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                        snapshot_tx.send_replace(Snapshot {
                            background_work: driver.has_background_work(),
                            fork_point: driver.fork_point().map(str::to_owned),
                            diagnostics: driver.diagnostic_snapshot(),
                        });
                        let failed = result.is_err();
                        let _ = request.reply.send(
                            result.map(|outcome| (outcome, activity.load(Ordering::Acquire))),
                        );
                        if failed {
                            break;
                        }
                    }
                    Request::Effort(effort, reply) => {
                        let result = driver.set_effort(effort.as_deref()).await;
                        let failed = result.is_err();
                        let _ = reply.send(result);
                        if failed {
                            break;
                        }
                    }
                    Request::Shutdown(reply) => {
                        let result = driver.shutdown().await;
                        snapshot_tx.send_modify(|snapshot| {
                            snapshot.diagnostics = driver.diagnostic_snapshot()
                        });
                        let _ = reply.send(result);
                        return;
                    }
                }
            }
            let _ = driver.shutdown().await;
            snapshot_tx.send_modify(|snapshot| snapshot.diagnostics = driver.diagnostic_snapshot());
        });
        Ok(Self {
            requests,
            continuations: Some(continuations),
            snapshot,
            mid_turn_delivery,
            generation,
            checkpoint,
            projected: AtomicU64::new(/*v*/ 0),
            completed_generation: 0,
            active_cancel,
            owner,
        })
    }

    pub fn take_continuations(&mut self) -> Option<mpsc::Receiver<ClaudeContinuation>> {
        self.continuations.take()
    }

    pub fn has_background_work(&self) -> bool {
        self.snapshot.borrow().background_work
    }

    pub fn has_unprojected_activity(&self) -> bool {
        self.projected.load(Ordering::Acquire) < self.generation.load(Ordering::Acquire)
    }

    pub fn set_checkpoint_invalidator(
        &self,
        invalidate: impl Fn() -> Result<()> + Send + Sync + 'static,
    ) {
        *self
            .checkpoint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(invalidate));
    }

    pub fn commit_projection(
        &self,
        generation: u64,
        commit: impl FnOnce(bool, Option<String>) -> Result<()>,
    ) -> Result<()> {
        let _publication = self
            .checkpoint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.projected.fetch_max(generation, Ordering::AcqRel);
        let snapshot = self.snapshot.borrow();
        commit(
            !snapshot.background_work && !self.has_unprojected_activity(),
            snapshot.fork_point.clone(),
        )
    }

    pub fn completed_generation(&self) -> u64 {
        self.completed_generation
    }

    pub fn fork_point(&self) -> Option<String> {
        self.snapshot.borrow().fork_point.clone()
    }

    pub fn subscribe_mid_turn_delivery(&self) -> watch::Receiver<bool> {
        self.mid_turn_delivery.clone()
    }

    pub fn diagnostic_snapshot(&self) -> Value {
        self.snapshot.borrow().diagnostics.clone()
    }

    #[tracing::instrument(skip_all)]
    pub async fn set_effort(&mut self, effort: Option<&str>) -> Result<()> {
        let (reply, result) = oneshot::channel();
        self.requests
            .send(Request::Effort(effort.map(str::to_owned), reply))
            .await
            .context("Claude owner stopped")?;
        result.await.context("Claude effort owner stopped")?
    }

    #[tracing::instrument(skip_all)]
    pub async fn run_turn(
        &mut self,
        prompt: &Prompt,
        events: mpsc::Sender<Value>,
        cancel: CancellationToken,
        steering: Option<mpsc::Receiver<Prompt>>,
    ) -> Result<ExternalTurnOutcome> {
        let (reply, result) = oneshot::channel();
        self.requests
            .send(Request::Turn(TurnRequest {
                prompt: prompt.clone(),
                events,
                cancel,
                steering,
                reply,
            }))
            .await
            .context("Claude owner stopped")?;
        let (outcome, generation) = result.await.context("Claude turn owner stopped")??;
        self.completed_generation = generation;
        Ok(outcome)
    }

    #[tracing::instrument(skip_all)]
    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(cancel) = self
            .active_cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            cancel.cancel();
        }
        let (reply, result) = oneshot::channel();
        if self.requests.send(Request::Shutdown(reply)).await.is_ok() {
            let result = result.await.context("Claude shutdown owner stopped")?;
            (&mut self.owner)
                .await
                .context("Claude owner task failed")?;
            result
        } else {
            (&mut self.owner)
                .await
                .context("Claude owner task failed")?;
            Ok(())
        }
    }
}

impl Drop for ClaudeRuntime {
    fn drop(&mut self) {
        self.owner.abort();
    }
}
