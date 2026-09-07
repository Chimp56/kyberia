//! Private macOS v1 DTOs. Foreign names never become domain types.
use serde::Deserialize;
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum E<T> {
    Known { value: T },
    Unknown { reason: Reason },
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reason {
    SourceDidNotProvide,
    NotCalibrated,
    UnsupportedSourceEnum,
    InvalidOrUnavailableSourceValue,
    NotSupportedByCollector,
    NotImplementedByCollector,
    NotCollected,
    NotRetainedByCollector,
    Redacted,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) enum Never {}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Time {
    pub receipt_utc: String,
    pub receipt_monotonic_ns: String,
    pub capture_time: E<Never>,
    pub clock_uncertainty_seconds: E<Never>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Body {
    Hello(Hello),
    Capabilities(Capabilities),
    Authorization(Authorization),
    ScanStarted(Start),
    ScanObservation(Box<Scan>),
    Complete(Complete),
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Hello {
    pub clock_epoch: String,
    pub collector: String,
    pub collector_build: String,
    pub collector_version: String,
    pub command: Command,
    pub evidence_origin: Origin,
    pub identifier_policy: Policy,
    pub max_observations: u16,
    pub max_record_bytes: u32,
    pub os_version: String,
    pub timeout_seconds: u8,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Command {
    Probe,
    Scan,
    Authorize,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Origin {
    NativeRuntime,
    SyntheticFixture,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Policy {
    Redacted,
    ExplicitUnredacted,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    pub source_id: String,
    pub interface_name: String,
    pub identity_scope: String,
    pub physical_radio_id: E<Never>,
    pub driver_version: E<Never>,
    pub firmware_version: E<Never>,
    pub collector: String,
    pub collector_version: String,
    pub collector_build: String,
    pub source_kind: String,
    pub source_api: String,
    pub source_schema: String,
    pub framework_version: String,
    pub os_version: String,
}
// Source listing uses the same provenance fields as observation source DTOs.
#[derive(Clone, Debug)]
pub(crate) struct ListedSource {
    pub source: Source,
    pub power_on: bool,
    pub supported_channel_count: E<u16>,
    pub reported_band_enums: Vec<u16>,
}
// Serde flatten does not enforce the nested source's unknown-field denial.
// Remove exactly the listing extension before deserializing the strict source.
impl<'de> Deserialize<'de> for ListedSource {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let serde_json::Value::Object(mut fields) = serde_json::Value::deserialize(d)? else {
            return Err(D::Error::custom("source object required"));
        };
        fn take<T: serde::de::DeserializeOwned, E: serde::de::Error>(
            fields: &mut serde_json::Map<String, serde_json::Value>,
            key: &str,
        ) -> Result<T, E> {
            serde_json::from_value(
                fields
                    .remove(key)
                    .ok_or_else(|| E::custom("missing source extension"))?,
            )
            .map_err(|_| E::custom("invalid source extension"))
        }
        let power_on = take(&mut fields, "power_on")?;
        let supported_channel_count = take(&mut fields, "supported_channel_count")?;
        let reported_band_enums = take(&mut fields, "reported_band_enums")?;
        let source = serde_json::from_value(serde_json::Value::Object(fields))
            .map_err(|_| D::Error::custom("invalid source fields"))?;
        Ok(Self {
            source,
            power_on,
            supported_channel_count,
            reported_band_enums,
        })
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Offer {
    pub state: String,
    pub condition: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Capabilities {
    pub location_services_enabled: bool,
    pub location_authorization: AuthorizationState,
    pub sources: Vec<ListedSource>,
    pub nearby_scan: Offer,
    pub noise_dbm: Offer,
    pub channel_width: Offer,
    pub monitor_frames: E<Never>,
    pub channel_dwell: E<Never>,
    pub channel_hopping_control: E<Never>,
    pub per_chain_signal: E<Never>,
    pub raw_payload_policy: String,
    pub phy_metadata: E<Never>,
    pub capture_timestamp: E<Never>,
    pub position: E<Never>,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthorizationState {
    Authorized,
    Denied,
    Restricted,
    NotDetermined,
    Unknown,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Authorization {
    pub state: AuthorizationState,
    pub requested_by_operator: bool,
    pub location_services_enabled: bool,
    pub prompt_requested: bool,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Start {
    pub api_started_monotonic_ns: String,
    pub include_hidden: bool,
    pub scan_id: String,
    pub source_id: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Window {
    pub start_monotonic_ns: String,
    pub end_monotonic_ns: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Channel {
    pub band: E<String>,
    pub center_frequency_hz: E<Never>,
    pub frequency_hz: E<Never>,
    pub puncturing: E<Never>,
    pub raw_band_enum: u16,
    pub raw_width_enum: u16,
    pub reported_channel_number: E<u16>,
    pub width_mhz: E<u16>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scan {
    pub api_window: Window,
    pub bssid: E<String>,
    pub calibration: String,
    pub channel: E<Channel>,
    pub dwell_seconds: E<Never>,
    pub evidence_class: String,
    pub information_elements: E<Never>,
    pub measurement_method: String,
    pub noise_dbm: E<i16>,
    pub observation_id: String,
    pub phy: E<Never>,
    pub position: E<Never>,
    pub quality: Vec<String>,
    pub result_age_seconds: E<Never>,
    pub rssi_dbm: E<i16>,
    pub scan_id: String,
    pub source: Source,
    pub ssid_octets_base64: E<String>,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalStatus {
    Ok,
    Partial,
    PermissionRequired,
    Unsupported,
    Unavailable,
    Error,
    Timeout,
    Cancelled,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Complete {
    pub observation_count: u16,
    pub partial: bool,
    pub reason: String,
    pub status: TerminalStatus,
    pub final_authorization: Option<AuthorizationState>,
    pub final_location_services_enabled: Option<bool>,
    pub native_error_domain: Option<String>,
    pub native_error_code: Option<i64>,
}
