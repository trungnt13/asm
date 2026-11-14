use crate::Prompt;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

pub struct ClaudeSteering {
    sender: mpsc::Sender<oneshot::Receiver<Prompt>>,
    closed: Arc<Mutex<bool>>,
}

pub(crate) enum Steering {
    Foreground(mpsc::Receiver<Prompt>),
    Continuation {
        receiver: mpsc::Receiver<oneshot::Receiver<Prompt>>,
        closed: Arc<Mutex<bool>>,
        pending: Option<oneshot::Receiver<Prompt>>,
    },
}

pub(crate) enum SteeringInput {
    Prompt(Prompt),
    Cancelled,
    Closed,
}

impl ClaudeSteering {
    pub(crate) fn channel() -> (Self, Steering) {
        let (sender, receiver) = mpsc::channel(/*buffer*/ 1);
        let closed = Arc::new(Mutex::new(/*t*/ false));
        (
            Self {
                sender,
                closed: Arc::clone(&closed),
            },
            Steering::Continuation {
                receiver,
                closed,
                pending: None,
            },
        )
    }

    pub fn is_ready(&self) -> bool {
        !*self
            .closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            && self.sender.capacity() > 0
    }

    pub fn reserve(&self) -> Option<oneshot::Sender<Prompt>> {
        let closed = self
            .closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *closed {
            return None;
        }
        let (sender, receiver) = oneshot::channel();
        self.sender.try_send(receiver).ok()?;
        Some(sender)
    }
}

impl Steering {
    #[tracing::instrument(skip_all)]
    pub(crate) async fn recv(&mut self) -> SteeringInput {
        match self {
            Self::Foreground(receiver) => match receiver.recv().await {
                Some(prompt) => SteeringInput::Prompt(prompt),
                None => SteeringInput::Closed,
            },
            Self::Continuation {
                receiver, pending, ..
            } => {
                if pending.is_none() {
                    *pending = receiver.recv().await;
                }
                let Some(reservation) = pending.as_mut() else {
                    return SteeringInput::Closed;
                };
                let delivered = reservation.await;
                *pending = None;
                match delivered {
                    Ok(prompt) => SteeringInput::Prompt(prompt),
                    Err(_) => SteeringInput::Cancelled,
                }
            }
        }
    }

    pub(crate) fn close_at_idle(&mut self) -> bool {
        match self {
            Self::Foreground(receiver) => receiver.is_empty(),
            Self::Continuation {
                receiver,
                closed,
                pending,
            } => {
                let mut closed = closed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if pending.is_some() || !receiver.is_empty() {
                    return false;
                }
                *closed = true;
                true
            }
        }
    }
}
