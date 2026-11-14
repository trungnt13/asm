use serde::Deserialize;
use serde::Serialize;

/// Direction of a local audio device, shared by enumeration and capture/playback selection.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioDeviceKind {
    Input,
    Output,
}

/// Local device preferences fixed for the lifetime of a voice conversation.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioDeviceSelection {
    pub microphone: Option<String>,
    pub speaker: Option<String>,
    pub channel: Option<Vec<std::num::NonZeroU16>>,
}

/// Local device metadata; enumeration never opens streams.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioDevice {
    pub name: String,
    pub channels: u16,
    pub is_default: bool,
}
