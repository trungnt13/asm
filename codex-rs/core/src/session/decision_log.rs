use super::session::Session;
use codex_api::DecisionsError;
use codex_protocol::ThreadId;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;
use serde::Serialize;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

const RECORD_BYTES: usize = 1024;
const QUEUE_RECORDS: usize = 64;
const FILE_BYTES: u64 = 4 * 1024 * 1024;
const DRAIN_TIME: Duration = Duration::from_millis(/*millis*/ 250);
const LOCK_WAIT: Duration = Duration::from_millis(/*millis*/ 500);
const WRITER_POLL: Duration = Duration::from_millis(/*millis*/ 25);

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DecisionPurpose {
    Warmup,
    Step,
}

pub(super) enum DecisionApiOutcome {
    Success,
    Error(DecisionsError),
    Timeout,
    Cancelled,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DecisionDisposition {
    Warmup,
    Applied,
    Unchanged,
    Rejected,
    Cancelled,
    Superseded,
    TargetUnavailable,
}

#[derive(Serialize)]
struct Record {
    timestamp_ms: u128,
    thread_id: ThreadId,
    attempt_id: uuid::Uuid,
    turn_id: Option<String>,
    purpose: DecisionPurpose,
    phase: &'static str,
    elapsed_ms: Option<u128>,
    http_attempted: bool,
    backend: Option<&'static str>,
    api: Option<&'static str>,
    error: Option<&'static str>,
    http_status: Option<u16>,
    disposition: Option<DecisionDisposition>,
    selected_effort: Option<ReasoningEffort>,
    current_effort: Option<ReasoningEffort>,
}

struct FailureNotice {
    reported: AtomicBool,
    session: Weak<Session>,
    runtime: tokio::runtime::Handle,
}

impl FailureNotice {
    fn report(&self) {
        if self.reported.swap(/*val*/ true, Ordering::AcqRel) {
            return;
        }
        let session = self.session.clone();
        self.runtime.spawn(async move {
            if let Some(session) = session.upgrade() {
                session.send_event_raw(Event {
                    id: String::new(),
                    msg: EventMsg::Warning(WarningEvent {
                        message: "Decisions diagnostic logging lost records or could not write decision.jsonl. Adaptive reasoning is unchanged.".to_owned(),
                    }),
                }).await;
            }
        });
    }
}

pub(super) struct DecisionLog {
    sender: mpsc::SyncSender<Vec<u8>>,
    failure: Arc<FailureNotice>,
    stopping: Arc<AtomicBool>,
    thread_id: ThreadId,
}

impl DecisionLog {
    // Callers gate construction on diagnostic_log_capture, including the queue and writer.
    pub(super) fn new(log_dir: &Path, thread_id: ThreadId, session: Weak<Session>) -> Arc<Self> {
        let (sender, receiver) = mpsc::sync_channel(/*bound*/ QUEUE_RECORDS);
        let failure = Arc::new(FailureNotice {
            reported: AtomicBool::new(/*v*/ false),
            session,
            runtime: tokio::runtime::Handle::current(),
        });
        let stopping = Arc::new(AtomicBool::new(/*v*/ false));
        let log_dir = log_dir.to_path_buf();
        let writer_failure = Arc::clone(&failure);
        let writer_stopping = Arc::clone(&stopping);
        let spawned = std::thread::Builder::new()
            .name("decision-log".to_owned())
            .spawn(move || {
                if write_records(&log_dir, thread_id, receiver, &writer_stopping).is_err() {
                    writer_failure.report();
                }
            });
        if spawned.is_err() {
            failure.report();
        }
        Arc::new(Self {
            sender,
            failure,
            stopping,
            thread_id,
        })
    }

    pub(super) fn start(
        self: &Arc<Self>,
        turn_id: Option<&str>,
        purpose: DecisionPurpose,
        retained: Option<ReasoningEffort>,
    ) -> DecisionAttempt {
        let record = Record {
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            thread_id: self.thread_id,
            attempt_id: uuid::Uuid::now_v7(),
            // Turn identifiers are internal, but remain capped before serialization.
            turn_id: turn_id.map(|id| id.chars().take(/*n*/ 128).collect()),
            purpose,
            phase: "start",
            elapsed_ms: None,
            http_attempted: false,
            backend: None,
            api: None,
            error: None,
            http_status: None,
            disposition: None,
            selected_effort: None,
            current_effort: retained,
        };
        self.record(&record);
        DecisionAttempt {
            log: Arc::clone(self),
            record,
            started: Instant::now(),
            finished: false,
        }
    }

    pub(super) fn close(&self) {
        // Session eviction must release the writer even when another owner retains the session.
        self.stopping.store(/*val*/ true, Ordering::Release);
    }

    fn record(&self, record: &Record) {
        let Ok(mut bytes) = serde_json::to_vec(record) else {
            self.failure.report();
            return;
        };
        bytes.push(b'\n');
        if bytes.len() > RECORD_BYTES || self.sender.try_send(bytes).is_err() {
            self.failure.report();
        }
    }
}

impl Drop for DecisionLog {
    fn drop(&mut self) {
        // No joins or filesystem operations on a model-step or shutdown path.
        self.close();
    }
}

pub(super) struct DecisionAttempt {
    log: Arc<DecisionLog>,
    record: Record,
    started: Instant,
    finished: bool,
}

impl DecisionAttempt {
    pub(super) fn mark_http_attempted(&mut self, backend: &'static str) {
        self.record.http_attempted = true;
        self.record.backend = Some(backend);
    }

    pub(super) fn finish(
        mut self,
        api: DecisionApiOutcome,
        disposition: DecisionDisposition,
        selected: Option<ReasoningEffort>,
    ) {
        self.complete(api, disposition, selected);
    }

    fn complete(
        &mut self,
        api: DecisionApiOutcome,
        disposition: DecisionDisposition,
        selected: Option<ReasoningEffort>,
    ) {
        self.record.phase = "end";
        self.record.timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        self.record.elapsed_ms = Some(self.started.elapsed().as_millis());
        if matches!(
            disposition,
            DecisionDisposition::Applied | DecisionDisposition::Unchanged
        ) {
            self.record.current_effort = selected.clone();
        }
        self.record.disposition = Some(disposition);
        self.record.selected_effort = selected;
        self.record.api = Some(match api {
            DecisionApiOutcome::Success => "success",
            DecisionApiOutcome::Timeout => "timeout",
            DecisionApiOutcome::Cancelled => "cancelled",
            DecisionApiOutcome::Error(error) => {
                self.record.error = Some(match error {
                    DecisionsError::Credentials => {
                        self.record.http_attempted = false;
                        "credentials"
                    }
                    DecisionsError::InvalidRequest => {
                        self.record.http_attempted = false;
                        "invalid_request"
                    }
                    DecisionsError::Transport => "transport",
                    DecisionsError::Http(status) => {
                        self.record.http_status = Some(status);
                        "http"
                    }
                    DecisionsError::ResponseTooLarge => "response_too_large",
                    DecisionsError::InvalidResponse => "invalid_response",
                });
                "error"
            }
        });
        self.log.record(&self.record);
        self.finished = true;
    }
}

impl Drop for DecisionAttempt {
    fn drop(&mut self) {
        if !self.finished {
            self.complete(
                DecisionApiOutcome::Cancelled,
                DecisionDisposition::Cancelled,
                /*selected*/ None,
            );
        }
    }
}

fn open_private_log(log_dir: &Path, thread_id: ThreadId) -> io::Result<File> {
    let mut directories = std::fs::DirBuilder::new();
    directories.recursive(/*recursive*/ true);
    let mut options = OpenOptions::new();
    // Windows file locking requires read or write access in addition to append access.
    options
        .read(/*read*/ true)
        .create(/*create*/ true)
        .append(/*append*/ true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        use std::os::unix::fs::OpenOptionsExt;
        directories.mode(0o700);
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Open the reparse point itself so a final-component link cannot redirect writes.
        options.custom_flags(/*flags*/ 0x0020_0000);
    }
    directories.create(log_dir)?;
    // The configured root is trusted; canonicalize normal OS aliases such as macOS /var.
    // Reject links only in the private directories and file owned by this logger.
    let parent = log_dir.canonicalize()?.join("adaptive-reasoning");
    let directory = parent.join(thread_id.to_string());
    for child in [&parent, &directory] {
        match std::fs::symlink_metadata(child) {
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Err(io::Error::other("unsafe diagnostic directory"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        directories.create(child)?;
    }
    let path = directory.join("decision.jsonl");
    if std::fs::symlink_metadata(&path)
        .is_ok_and(|metadata| !metadata.is_file() || metadata.file_type().is_symlink())
    {
        return Err(io::Error::other("unsafe diagnostic file"));
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("nonregular diagnostic file"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::other("diagnostic reparse point"));
        }
    }
    // Eviction may retain the old session briefly. Only the writer waits for its file handoff.
    let lock_started = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if lock_started.elapsed() < LOCK_WAIT => {
                std::thread::sleep(WRITER_POLL);
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(io::Error::other("diagnostic writer handoff expired"));
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;
        if file.metadata()?.nlink() != 1 {
            return Err(io::Error::other("linked diagnostic file"));
        }
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn write_records(
    log_dir: &Path,
    thread_id: ThreadId,
    receiver: mpsc::Receiver<Vec<u8>>,
    stopping: &AtomicBool,
) -> io::Result<()> {
    let mut file = open_private_log(log_dir, thread_id)?;
    let mut bytes = file.metadata()?.len();
    let mut drain_started = None;
    loop {
        let record = if stopping.load(Ordering::Acquire) {
            let started = drain_started.get_or_insert_with(Instant::now);
            match receiver.try_recv() {
                Ok(record) => {
                    if started.elapsed() >= DRAIN_TIME {
                        return Err(io::Error::other("diagnostic drain expired"));
                    }
                    record
                }
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        } else {
            match receiver.recv_timeout(/*timeout*/ WRITER_POLL) {
                Ok(record) => record,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        };
        bytes = bytes.saturating_add(record.len() as u64);
        if bytes > FILE_BYTES {
            return Err(io::Error::other("diagnostic storage limit reached"));
        }
        file.write_all(&record)?;
    }
}
