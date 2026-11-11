#[cfg(feature = "terminal-voice")]
mod client;
mod helper_exit;
#[cfg(all(feature = "terminal-voice", any(target_os = "linux", test)))]
mod linux_alsa;
#[cfg(feature = "terminal-voice")]
mod message_reader;
mod protocol;
#[cfg(feature = "terminal-voice")]
mod session;
#[cfg(not(feature = "terminal-voice"))]
mod unsupported;

#[cfg(feature = "terminal-voice")]
pub use client::ConnectionError;
#[cfg(feature = "terminal-voice")]
pub use client::VoiceHost;
pub use helper_exit::HelperExitStage;
pub use protocol::AudioControls;
pub use protocol::AudioDevice;
pub use protocol::AudioDeviceKind;
pub use protocol::AudioDeviceSelection;
pub use protocol::AudioState;
pub use protocol::MAX_FRAME_BYTES;
pub use protocol::Message;
pub use protocol::RUNTIME_ENVIRONMENT;
pub use protocol::SessionDescription;
pub use protocol::decode_frame;
pub use protocol::encode_frame;
pub use protocol::read_message;
#[cfg(feature = "terminal-voice")]
pub use session::RealtimeWebrtcSession;
#[cfg(feature = "terminal-voice")]
pub use session::RealtimeWebrtcSessionHandle;
#[cfg(feature = "terminal-voice")]
pub use session::StartedRealtimeWebrtcSession;

#[cfg(not(feature = "terminal-voice"))]
pub use unsupported::ConnectionError;
#[cfg(not(feature = "terminal-voice"))]
pub use unsupported::RealtimeWebrtcSession;
#[cfg(not(feature = "terminal-voice"))]
pub use unsupported::RealtimeWebrtcSessionHandle;
#[cfg(not(feature = "terminal-voice"))]
pub use unsupported::StartedRealtimeWebrtcSession;
#[cfg(not(feature = "terminal-voice"))]
pub use unsupported::VoiceHost;
