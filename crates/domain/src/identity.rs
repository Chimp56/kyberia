//! Opaque 128-bit canonical IDs. The application supplies collision-resistant
//! IDs (UUIDv7 bytes are suitable); generation is an outer-layer side effect.
//! IDs serialize as 32 lowercase hexadecimal digits. MACs are evidence, not IDs.
use crate::ValidationError;
use serde::{Deserialize, Serialize};

fn decode_hex<const N: usize>(text: &str) -> Result<[u8; N], ValidationError> {
    if text.len() != N * 2
        || !text
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(ValidationError::InvalidIdentity);
    }
    let mut bytes = [0; N];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)
            .map_err(|_| ValidationError::InvalidIdentity)?;
    }
    Ok(bytes)
}
fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

macro_rules! identifier {
    ($($name:ident),+ $(,)?) => {$ (
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name([u8; 16]);
        impl $name {
            pub fn from_bytes(bytes: [u8; 16]) -> Result<Self, ValidationError> {
                if bytes == [0;16] { return Err(ValidationError::InvalidIdentity); }
                Ok(Self(bytes))
            }
            pub const fn bytes(self) -> [u8;16] { self.0 }
        }
        impl TryFrom<String> for $name {
            type Error = ValidationError;
            fn try_from(s: String) -> Result<Self, Self::Error> { Self::from_bytes(decode_hex(&s)?) }
        }
        impl From<$name> for String { fn from(id: $name) -> Self { encode_hex(&id.0) } }
    )+}
}
identifier!(
    ProjectId,
    SiteId,
    BuildingId,
    FloorId,
    FrameId,
    ObservationId,
    SessionId,
    SnapshotId,
    SourceId,
    SensorId,
    AdapterId,
    CollectorId,
    ClockEpochId,
    CalibrationId,
    PoseId,
    PhysicalDeviceId,
    RadioId,
    BssId,
    EssId,
    MldId,
    ClientId,
    AnalysisRunId,
    ActorId,
    OperationId,
    ChannelScheduleId,
    EndpointId
);

/// A content reference identifies bytes, never a filesystem path or URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ContentHash([u8; 32]);
impl ContentHash {
    pub const fn from_sha256(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}
impl TryFrom<String> for ContentHash {
    type Error = ValidationError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Ok(Self(decode_hex(&s)?))
    }
}
impl From<ContentHash> for String {
    fn from(id: ContentHash) -> Self {
        encode_hex(&id.0)
    }
}

/// Bounded human-readable metadata; arbitrary binary SSIDs use Ssid instead.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Text(String);
impl Text {
    pub fn new(s: impl Into<String>) -> Result<Self, ValidationError> {
        let s = s.into();
        if s.trim().is_empty() || s.len() > 1024 || s.chars().any(char::is_control) {
            return Err(ValidationError::InvalidText);
        }
        Ok(Self(s))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for Text {
    type Error = ValidationError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}
impl From<Text> for String {
    fn from(t: Text) -> Self {
        t.0
    }
}

/// Up to 32 octets; empty and non-UTF8 SSIDs remain valid evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<u8>", into = "Vec<u8>")]
pub struct Ssid(Vec<u8>);
impl Ssid {
    pub fn new(bytes: Vec<u8>) -> Result<Self, ValidationError> {
        if bytes.len() > 32 {
            return Err(ValidationError::ResourceLimit("ssid"));
        }
        Ok(Self(bytes))
    }
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}
impl TryFrom<Vec<u8>> for Ssid {
    type Error = ValidationError;
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        Self::new(bytes)
    }
}
impl From<Ssid> for Vec<u8> {
    fn from(ssid: Ssid) -> Self {
        ssid.0
    }
}

/// Link-layer address evidence; preserves any six-octet source value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacAddress(pub [u8; 6]);
