use crate::AudioDevice;
use crate::AudioDeviceKind;
use crate::AudioDeviceSelection;
use anyhow::Result;
use futures::future::AbortRegistration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionError {
    NegotiationTimedOut,
    Failed,
    HelperStartup,
    RuntimeInitialization,
    Transport,
    AudioDevices,
    AudioControls,
    AudioSession,
    Shutdown,
}
impl std::fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NegotiationTimedOut => "voice negotiation timed out",
            Self::Failed => "voice connection failed",
            Self::HelperStartup => "voice helper could not start",
            Self::RuntimeInitialization => "voice audio runtime could not initialize",
            Self::Transport => "voice transport could not connect",
            Self::AudioDevices => {
                "voice audio devices could not open; check microphone and speaker setup"
            }
            Self::AudioControls => "voice audio controls failed",
            Self::AudioSession => "voice audio session stopped unexpectedly",
            Self::Shutdown => "voice helper could not shut down cleanly",
        })
    }
}
impl std::error::Error for ConnectionError {}

#[derive(Clone, Debug)]
pub struct RealtimeWebrtcSessionHandle(std::convert::Infallible);

#[derive(Debug)]
pub struct StartedRealtimeWebrtcSession {
    pub offer_sdp: String,
    pub handle: RealtimeWebrtcSessionHandle,
}

pub struct RealtimeWebrtcSession;

impl RealtimeWebrtcSession {
    pub fn is_supported() -> bool {
        false
    }
    pub fn start(
        _abort: AbortRegistration,
        _selection: AudioDeviceSelection,
    ) -> Result<StartedRealtimeWebrtcSession> {
        anyhow::bail!("terminal voice is not included in this ASM build")
    }
}

impl RealtimeWebrtcSessionHandle {
    pub fn close(&self) {
        match self.0 {}
    }
    pub fn set_microphone_muted(&self, _muted: bool) -> Result<()> {
        match self.0 {}
    }
    pub fn set_speaker_suppressed(&self, _suppressed: bool) {
        match self.0 {}
    }
    pub fn apply_answer_sdp(&self, _answer: String) -> std::result::Result<(), ConnectionError> {
        match self.0 {}
    }
    pub fn take_error(&self) -> Option<(ConnectionError, String)> {
        match self.0 {}
    }
    pub fn take_microphone_peak(&self) -> u16 {
        match self.0 {}
    }
    pub fn take_speaker_peak(&self) -> u16 {
        match self.0 {}
    }
}

pub struct VoiceHost(std::convert::Infallible);

impl VoiceHost {
    pub async fn connect(
        _package: &codex_install_context::CodexPackageLayout,
        _build_commit: &str,
    ) -> Result<Self> {
        anyhow::bail!("terminal voice is not included in this ASM build")
    }
    pub async fn list_devices(&mut self, _kind: AudioDeviceKind) -> Result<Vec<AudioDevice>> {
        match self.0 {}
    }
    pub async fn close(self) -> Result<()> {
        match self.0 {}
    }
}
