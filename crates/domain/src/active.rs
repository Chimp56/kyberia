//! Canonical contracts for bounded, consented active measurements.
//!
//! This module deliberately describes one safe measurement method: TCP
//! connection timing to a literal IP address.  It does not describe ICMP,
//! application latency, Wi-Fi airtime, throughput, DNS, or an Internet
//! experience.  Network I/O lives in `kyberia-active-measurement`; these
//! records contain only validated values and provenance.

use crate::{
    ValidationError,
    evidence::{Evidence, UnknownReason},
    identity::{
        ActiveEndpointId, ActiveIntervalId, ActiveResultId, ActiveSampleId, ActiveTestRunId,
        AdapterId, ClockEpochId, MacAddress, SensorId, Text,
    },
    time::{MonotonicTimestamp, MonotonicWindow},
    units::{Milliseconds, Percentage, Seconds},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const ACTIVE_SCHEMA_METHOD_VERSION: &str = "rf-atlas-active-tcp-connect-timing/v1";
pub const MAX_ACTIVE_ENDPOINTS: usize = 32;
pub const MAX_ACTIVE_SAMPLES: u32 = 4_096;
pub const MAX_ACTIVE_CONCURRENCY: u16 = 8;
pub const MAX_ACTIVE_TIMEOUT_SECONDS: f64 = 60.0;
pub const MAX_ACTIVE_DURATION_SECONDS: f64 = 3_600.0;
pub const MAX_ACTIVE_SPACING_SECONDS: f64 = 3_600.0;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveTimestampWire {
    epoch: ClockEpochId,
    nanoseconds: u64,
}

impl ActiveTimestampWire {
    fn into_timestamp(self) -> MonotonicTimestamp {
        MonotonicTimestamp {
            epoch: self.epoch,
            nanoseconds: self.nanoseconds,
        }
    }
}

impl From<MonotonicTimestamp> for ActiveTimestampWire {
    fn from(value: MonotonicTimestamp) -> Self {
        Self {
            epoch: value.epoch,
            nanoseconds: value.nanoseconds,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveWindowWire {
    start: ActiveTimestampWire,
    end: ActiveTimestampWire,
}

impl ActiveWindowWire {
    fn into_window(self) -> Result<MonotonicWindow, ValidationError> {
        MonotonicWindow::new(self.start.into_timestamp(), self.end.into_timestamp())
    }
}

impl From<MonotonicWindow> for ActiveWindowWire {
    fn from(value: MonotonicWindow) -> Self {
        Self {
            start: value.start().into(),
            end: value.end().into(),
        }
    }
}

/// Compatibility names for callers that use the shorter record terminology.
pub type ActiveRunId = ActiveTestRunId;
pub type ActiveEndpointIdentity = ActiveEndpointId;
pub type ActiveIntervalIdentity = ActiveIntervalId;
pub type ActiveSampleIdentity = ActiveSampleId;
pub type ActiveResultIdentity = ActiveResultId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActiveSchemaVersion {
    #[serde(rename = "1")]
    V1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveEndpointTier {
    Gateway,
    LanReference,
    InternetControl,
    Application,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveTransportProtocol {
    Tcp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveMeasurementMethod {
    TcpConnectTiming,
}

impl ActiveMeasurementMethod {
    pub const fn protocol(self) -> ActiveTransportProtocol {
        ActiveTransportProtocol::Tcp
    }

    pub const fn semantic_name(self) -> &'static str {
        "TCP connect timing"
    }
}

/// An IP address represented without a resolver or hostname.  A literal
/// address is required so a measurement cannot silently change destination
/// because DNS or a search suffix changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveIpAddress {
    V4([u8; 4]),
    V6([u8; 16]),
}

impl ActiveIpAddress {
    pub const fn v4(octets: [u8; 4]) -> Self {
        Self::V4(octets)
    }

    pub const fn v6(octets: [u8; 16]) -> Self {
        Self::V6(octets)
    }

    pub fn is_unspecified(self) -> bool {
        match self {
            Self::V4(octets) => octets == [0, 0, 0, 0],
            Self::V6(octets) => octets == [0; 16],
        }
    }

    pub const fn is_loopback(self) -> bool {
        match self {
            Self::V4(octets) => octets[0] == 127,
            Self::V6(octets) => {
                let mut index = 0;
                while index < 15 {
                    if octets[index] != 0 {
                        return false;
                    }
                    index += 1;
                }
                octets[15] == 1
            }
        }
    }

    pub const fn is_multicast(self) -> bool {
        match self {
            Self::V4(octets) => octets[0] >= 224,
            Self::V6(octets) => octets[0] == 0xff,
        }
    }

    pub const fn is_broadcast(self) -> bool {
        matches!(self, Self::V4([255, 255, 255, 255]))
    }

    pub const fn is_link_local(self) -> bool {
        match self {
            Self::V4(octets) => octets[0] == 169 && octets[1] == 254,
            Self::V6(octets) => octets[0] == 0xfe && (octets[1] & 0xc0) == 0x80,
        }
    }

    /// IPv4-mapped IPv6 addresses must not bypass IPv4 target safety checks.
    /// They are rejected at the socket-address boundary instead of being
    /// treated as ordinary IPv6 unicast addresses.
    pub const fn is_ipv4_mapped(self) -> bool {
        match self {
            Self::V4(_) => false,
            Self::V6(octets) => {
                let mut index = 0;
                while index < 10 {
                    if octets[index] != 0 {
                        return false;
                    }
                    index += 1;
                }
                octets[10] == 0xff && octets[11] == 0xff
            }
        }
    }

    /// Private/ULA ranges are suitable for the local gateway and LAN tiers.
    /// This intentionally excludes loopback and link-local so those require a
    /// separate explicit authorization below.
    pub const fn is_private_local(self) -> bool {
        match self {
            Self::V4(octets) => {
                (octets[0] == 10)
                    || (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31)
                    || (octets[0] == 192 && octets[1] == 168)
            }
            Self::V6(octets) => (octets[0] & 0xfe) == 0xfc,
        }
    }

    pub const fn is_local_only(self) -> bool {
        self.is_loopback() || self.is_link_local() || self.is_private_local()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "ActiveSocketAddrWire", into = "ActiveSocketAddrWire")]
pub struct ActiveSocketAddr {
    address: ActiveIpAddress,
    /// Literal TCP destination port.  Port zero is rejected and no service
    /// name or protocol other than TCP is resolved by this contract.
    port: u16,
}

impl ActiveSocketAddr {
    pub fn new(address: ActiveIpAddress, port: u16) -> Result<Self, ValidationError> {
        if port == 0 {
            return Err(ValidationError::OutOfRange("active TCP port"));
        }
        if address.is_unspecified()
            || address.is_multicast()
            || address.is_broadcast()
            || address.is_ipv4_mapped()
            || (matches!(address, ActiveIpAddress::V6(_)) && address.is_link_local())
        {
            return Err(ValidationError::OutOfRange(
                "active TCP unicast address (IPv6 link-local unsupported in this schema)",
            ));
        }
        Ok(Self { address, port })
    }

    pub const fn address(self) -> ActiveIpAddress {
        self.address
    }

    pub const fn port(self) -> u16 {
        self.port
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveSocketAddrWire {
    address: ActiveIpAddress,
    port: u16,
}

impl TryFrom<ActiveSocketAddrWire> for ActiveSocketAddr {
    type Error = ValidationError;

    fn try_from(value: ActiveSocketAddrWire) -> Result<Self, Self::Error> {
        Self::new(value.address, value.port)
    }
}

impl From<ActiveSocketAddr> for ActiveSocketAddrWire {
    fn from(value: ActiveSocketAddr) -> Self {
        Self {
            address: value.address,
            port: value.port,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ActiveTargetWire", into = "ActiveTargetWire")]
pub struct ActiveTarget {
    identity: Text,
    tier: ActiveEndpointTier,
    address: ActiveSocketAddr,
}

impl ActiveTarget {
    pub fn new(
        identity: Text,
        tier: ActiveEndpointTier,
        address: ActiveSocketAddr,
    ) -> Result<Self, ValidationError> {
        if address.address().is_unspecified()
            || address.address().is_multicast()
            || address.address().is_broadcast()
            || address.address().is_ipv4_mapped()
        {
            return Err(ValidationError::OutOfRange("active target address"));
        }
        Ok(Self {
            identity,
            tier,
            address,
        })
    }

    pub fn identity(&self) -> &Text {
        &self.identity
    }

    pub const fn tier(&self) -> ActiveEndpointTier {
        self.tier
    }

    pub const fn address(&self) -> ActiveSocketAddr {
        self.address
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveTargetWire {
    identity: Text,
    tier: ActiveEndpointTier,
    address: ActiveSocketAddr,
}

impl TryFrom<ActiveTargetWire> for ActiveTarget {
    type Error = ValidationError;

    fn try_from(value: ActiveTargetWire) -> Result<Self, Self::Error> {
        Self::new(value.identity, value.tier, value.address)
    }
}

impl From<ActiveTarget> for ActiveTargetWire {
    fn from(value: ActiveTarget) -> Self {
        Self {
            identity: value.identity,
            tier: value.tier,
            address: value.address,
        }
    }
}

/// Attribution is evidence, including when the operating system cannot
/// expose the active interface, route, or associated BSSID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointAttribution {
    interface: Evidence<Text>,
    route: Evidence<Text>,
    bssid: Evidence<MacAddress>,
}

impl EndpointAttribution {
    pub const fn new(
        interface: Evidence<Text>,
        route: Evidence<Text>,
        bssid: Evidence<MacAddress>,
    ) -> Self {
        Self {
            interface,
            route,
            bssid,
        }
    }

    pub const fn interface(&self) -> &Evidence<Text> {
        &self.interface
    }

    pub const fn route(&self) -> &Evidence<Text> {
        &self.route
    }

    pub const fn bssid(&self) -> &Evidence<MacAddress> {
        &self.bssid
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ActiveProvenanceWire", into = "ActiveProvenanceWire")]
pub struct ActiveMeasurementProvenance {
    source: Text,
    source_version: Text,
    method_version: Text,
    clock_epoch: ClockEpochId,
    adapter: Evidence<AdapterId>,
    sensor: Evidence<SensorId>,
}

impl ActiveMeasurementProvenance {
    pub fn new(
        source: Text,
        source_version: Text,
        method_version: Text,
        clock_epoch: ClockEpochId,
        adapter: Evidence<AdapterId>,
        sensor: Evidence<SensorId>,
    ) -> Result<Self, ValidationError> {
        if method_version.as_str() != ACTIVE_SCHEMA_METHOD_VERSION {
            return Err(ValidationError::UnsupportedSchema);
        }
        Ok(Self {
            source,
            source_version,
            method_version,
            clock_epoch,
            adapter,
            sensor,
        })
    }

    pub fn source(&self) -> &Text {
        &self.source
    }

    pub fn source_version(&self) -> &Text {
        &self.source_version
    }

    pub fn method_version(&self) -> &Text {
        &self.method_version
    }

    pub const fn clock_epoch(&self) -> ClockEpochId {
        self.clock_epoch
    }

    pub const fn adapter(&self) -> &Evidence<AdapterId> {
        &self.adapter
    }

    pub const fn sensor(&self) -> &Evidence<SensorId> {
        &self.sensor
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveProvenanceWire {
    source: Text,
    source_version: Text,
    method_version: Text,
    clock_epoch: ClockEpochId,
    adapter: Evidence<AdapterId>,
    sensor: Evidence<SensorId>,
}

impl TryFrom<ActiveProvenanceWire> for ActiveMeasurementProvenance {
    type Error = ValidationError;

    fn try_from(value: ActiveProvenanceWire) -> Result<Self, Self::Error> {
        Self::new(
            value.source,
            value.source_version,
            value.method_version,
            value.clock_epoch,
            value.adapter,
            value.sensor,
        )
    }
}

impl From<ActiveMeasurementProvenance> for ActiveProvenanceWire {
    fn from(value: ActiveMeasurementProvenance) -> Self {
        Self {
            source: value.source,
            source_version: value.source_version,
            method_version: value.method_version,
            clock_epoch: value.clock_epoch,
            adapter: value.adapter,
            sensor: value.sensor,
        }
    }
}

/// User authorization is a required input to topology validation.  The
/// allow-list is deliberately tier-specific; there is no global wildcard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ActiveAuthorizationWire", into = "ActiveAuthorizationWire")]
pub struct ActiveAuthorization {
    user_consent: bool,
    authorized_tiers: Vec<ActiveEndpointTier>,
    allow_loopback: bool,
    allow_link_local: bool,
    purpose: Text,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveAuthorizationWire {
    user_consent: bool,
    authorized_tiers: Vec<ActiveEndpointTier>,
    allow_loopback: bool,
    allow_link_local: bool,
    purpose: Text,
}

impl TryFrom<ActiveAuthorizationWire> for ActiveAuthorization {
    type Error = ValidationError;

    fn try_from(value: ActiveAuthorizationWire) -> Result<Self, Self::Error> {
        Self::new(
            value.user_consent,
            value.authorized_tiers,
            value.allow_loopback,
            value.allow_link_local,
            value.purpose,
        )
    }
}

impl From<ActiveAuthorization> for ActiveAuthorizationWire {
    fn from(value: ActiveAuthorization) -> Self {
        Self {
            user_consent: value.user_consent,
            authorized_tiers: value.authorized_tiers,
            allow_loopback: value.allow_loopback,
            allow_link_local: value.allow_link_local,
            purpose: value.purpose,
        }
    }
}

impl ActiveAuthorization {
    pub fn new(
        user_consent: bool,
        authorized_tiers: Vec<ActiveEndpointTier>,
        allow_loopback: bool,
        allow_link_local: bool,
        purpose: Text,
    ) -> Result<Self, ValidationError> {
        if authorized_tiers.is_empty() || authorized_tiers.len() > 4 {
            return Err(ValidationError::ResourceLimit("active authorization tiers"));
        }
        let mut unique = BTreeSet::new();
        if authorized_tiers.iter().any(|tier| !unique.insert(*tier)) {
            return Err(ValidationError::Inconsistent(
                "duplicate active authorization tier",
            ));
        }
        Ok(Self {
            user_consent,
            authorized_tiers,
            allow_loopback,
            allow_link_local,
            purpose,
        })
    }

    pub const fn user_consent(&self) -> bool {
        self.user_consent
    }

    pub fn authorized_tiers(&self) -> &[ActiveEndpointTier] {
        &self.authorized_tiers
    }

    pub const fn allow_loopback(&self) -> bool {
        self.allow_loopback
    }

    pub const fn allow_link_local(&self) -> bool {
        self.allow_link_local
    }

    pub fn purpose(&self) -> &Text {
        &self.purpose
    }

    pub fn permits(&self, tier: ActiveEndpointTier) -> bool {
        self.user_consent && self.authorized_tiers.contains(&tier)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActiveValidationError {
    Domain(ValidationError),
    AuthorizationRequired,
    UnsafeTarget,
    TargetTierMismatch,
    UnsupportedMethod,
}

impl std::fmt::Display for ActiveValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Domain(error) => write!(f, "active domain validation failed: {error}"),
            Self::AuthorizationRequired => f.write_str("active measurement authorization required"),
            Self::UnsafeTarget => f.write_str("unsafe active measurement target"),
            Self::TargetTierMismatch => f.write_str("active target and endpoint tier mismatch"),
            Self::UnsupportedMethod => f.write_str("unsupported active measurement method"),
        }
    }
}

impl std::error::Error for ActiveValidationError {}

impl From<ValidationError> for ActiveValidationError {
    fn from(error: ValidationError) -> Self {
        Self::Domain(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ActiveEndpointWire", into = "ActiveEndpointWire")]
pub struct ActiveEndpoint {
    schema_version: ActiveSchemaVersion,
    id: ActiveEndpointId,
    tier: ActiveEndpointTier,
    target: ActiveTarget,
    protocol: ActiveTransportProtocol,
    method: ActiveMeasurementMethod,
    attribution: EndpointAttribution,
}

impl ActiveEndpoint {
    pub fn new<I: Into<ActiveEndpointId>>(
        id: I,
        tier: ActiveEndpointTier,
        target: ActiveTarget,
        protocol: ActiveTransportProtocol,
        method: ActiveMeasurementMethod,
        attribution: EndpointAttribution,
    ) -> Result<Self, ActiveValidationError> {
        if target.tier() != tier {
            return Err(ActiveValidationError::TargetTierMismatch);
        }
        if protocol != method.protocol() {
            return Err(ActiveValidationError::UnsupportedMethod);
        }
        Ok(Self {
            schema_version: ActiveSchemaVersion::V1,
            id: id.into(),
            tier,
            target,
            protocol,
            method,
            attribution,
        })
    }

    pub const fn id(&self) -> ActiveEndpointId {
        self.id
    }

    pub const fn tier(&self) -> ActiveEndpointTier {
        self.tier
    }

    pub fn target(&self) -> &ActiveTarget {
        &self.target
    }

    pub const fn protocol(&self) -> ActiveTransportProtocol {
        self.protocol
    }

    pub const fn method(&self) -> ActiveMeasurementMethod {
        self.method
    }

    pub const fn attribution(&self) -> &EndpointAttribution {
        &self.attribution
    }

    pub fn validate_for_authorization(
        &self,
        authorization: &ActiveAuthorization,
    ) -> Result<(), ActiveValidationError> {
        if !authorization.permits(self.tier) {
            return Err(ActiveValidationError::AuthorizationRequired);
        }
        let address = self.target.address().address();
        if address.is_unspecified() || address.is_multicast() || address.is_broadcast() {
            return Err(ActiveValidationError::UnsafeTarget);
        }
        if address.is_loopback()
            && (!authorization.allow_loopback()
                || !matches!(
                    self.tier,
                    ActiveEndpointTier::Gateway | ActiveEndpointTier::LanReference
                ))
        {
            return Err(ActiveValidationError::UnsafeTarget);
        }
        if address.is_link_local()
            && (!authorization.allow_link_local()
                || !matches!(
                    self.tier,
                    ActiveEndpointTier::Gateway | ActiveEndpointTier::LanReference
                ))
        {
            return Err(ActiveValidationError::UnsafeTarget);
        }
        match self.tier {
            ActiveEndpointTier::Gateway | ActiveEndpointTier::LanReference
                if !address.is_local_only() =>
            {
                Err(ActiveValidationError::TargetTierMismatch)
            }
            ActiveEndpointTier::InternetControl if address.is_local_only() => {
                Err(ActiveValidationError::TargetTierMismatch)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveEndpointWire {
    schema_version: ActiveSchemaVersion,
    id: ActiveEndpointId,
    tier: ActiveEndpointTier,
    target: ActiveTarget,
    protocol: ActiveTransportProtocol,
    method: ActiveMeasurementMethod,
    attribution: EndpointAttribution,
}

impl TryFrom<ActiveEndpointWire> for ActiveEndpoint {
    type Error = ActiveValidationError;

    fn try_from(value: ActiveEndpointWire) -> Result<Self, Self::Error> {
        if value.schema_version != ActiveSchemaVersion::V1 {
            return Err(ActiveValidationError::Domain(
                ValidationError::UnsupportedSchema,
            ));
        }
        Self::new(
            value.id,
            value.tier,
            value.target,
            value.protocol,
            value.method,
            value.attribution,
        )
    }
}

impl From<ActiveEndpoint> for ActiveEndpointWire {
    fn from(value: ActiveEndpoint) -> Self {
        Self {
            schema_version: value.schema_version,
            id: value.id,
            tier: value.tier,
            target: value.target,
            protocol: value.protocol,
            method: value.method,
            attribution: value.attribution,
        }
    }
}

/// Resource and impact limits for one run.  A caller may choose a smaller
/// value, but cannot raise these hard upper bounds through configuration.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ActiveLimitsWire", into = "ActiveLimitsWire")]
pub struct ActiveTestLimits {
    max_samples_total: u32,
    max_concurrency: u16,
    per_attempt_timeout: Seconds,
    overall_duration: Seconds,
    minimum_spacing: Seconds,
}

impl ActiveTestLimits {
    pub fn new(
        max_samples_total: u32,
        max_concurrency: u16,
        per_attempt_timeout: Seconds,
        overall_duration: Seconds,
        minimum_spacing: Seconds,
    ) -> Result<Self, ValidationError> {
        if max_samples_total == 0 || max_samples_total > MAX_ACTIVE_SAMPLES {
            return Err(ValidationError::ResourceLimit("active sample count"));
        }
        if max_concurrency == 0 || max_concurrency > MAX_ACTIVE_CONCURRENCY {
            return Err(ValidationError::ResourceLimit("active concurrency"));
        }
        if per_attempt_timeout.get() <= 0.0
            || per_attempt_timeout.get() > MAX_ACTIVE_TIMEOUT_SECONDS
            || overall_duration.get() <= 0.0
            || overall_duration.get() > MAX_ACTIVE_DURATION_SECONDS
            || minimum_spacing.get() < 0.0
            || minimum_spacing.get() > MAX_ACTIVE_SPACING_SECONDS
        {
            return Err(ValidationError::OutOfRange("active timing limits"));
        }
        if per_attempt_timeout.get() > overall_duration.get() {
            return Err(ValidationError::Inconsistent(
                "active attempt timeout exceeds run duration",
            ));
        }
        if seconds_to_nanos(per_attempt_timeout)? == 0
            || seconds_to_nanos(overall_duration)? == 0
            || (minimum_spacing.get() > 0.0 && seconds_to_nanos(minimum_spacing)? == 0)
        {
            return Err(ValidationError::OutOfRange("active timing precision"));
        }
        Ok(Self {
            max_samples_total,
            max_concurrency,
            per_attempt_timeout,
            overall_duration,
            minimum_spacing,
        })
    }

    pub const fn max_samples_total(self) -> u32 {
        self.max_samples_total
    }

    pub const fn max_concurrency(self) -> u16 {
        self.max_concurrency
    }

    pub const fn per_attempt_timeout(self) -> Seconds {
        self.per_attempt_timeout
    }

    pub const fn overall_duration(self) -> Seconds {
        self.overall_duration
    }

    pub const fn minimum_spacing(self) -> Seconds {
        self.minimum_spacing
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveLimitsWire {
    max_samples_total: u32,
    max_concurrency: u16,
    per_attempt_timeout: Seconds,
    overall_duration: Seconds,
    minimum_spacing: Seconds,
}

impl TryFrom<ActiveLimitsWire> for ActiveTestLimits {
    type Error = ValidationError;

    fn try_from(value: ActiveLimitsWire) -> Result<Self, Self::Error> {
        Self::new(
            value.max_samples_total,
            value.max_concurrency,
            value.per_attempt_timeout,
            value.overall_duration,
            value.minimum_spacing,
        )
    }
}

impl From<ActiveTestLimits> for ActiveLimitsWire {
    fn from(value: ActiveTestLimits) -> Self {
        Self {
            max_samples_total: value.max_samples_total,
            max_concurrency: value.max_concurrency,
            per_attempt_timeout: value.per_attempt_timeout,
            overall_duration: value.overall_duration,
            minimum_spacing: value.minimum_spacing,
        }
    }
}

fn seconds_to_nanos(value: Seconds) -> Result<u64, ValidationError> {
    let nanos = value.get() * 1e9;
    if !nanos.is_finite() || nanos < 0.0 || nanos > u64::MAX as f64 {
        return Err(ValidationError::OutOfRange("active nanoseconds"));
    }
    Ok(nanos.round() as u64)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ActiveTestRunWire", into = "ActiveTestRunWire")]
pub struct ActiveTestRun {
    schema_version: ActiveSchemaVersion,
    id: ActiveTestRunId,
    started: MonotonicTimestamp,
    deadline: MonotonicTimestamp,
    endpoints: Vec<ActiveEndpoint>,
    intervals: Vec<ActiveInterval>,
    authorization: ActiveAuthorization,
    limits: ActiveTestLimits,
    provenance: ActiveMeasurementProvenance,
}

impl ActiveTestRun {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ActiveTestRunId,
        started: MonotonicTimestamp,
        deadline: MonotonicTimestamp,
        endpoints: Vec<ActiveEndpoint>,
        authorization: ActiveAuthorization,
        limits: ActiveTestLimits,
        provenance: ActiveMeasurementProvenance,
    ) -> Result<Self, ActiveValidationError> {
        if endpoints.is_empty() || endpoints.len() > MAX_ACTIVE_ENDPOINTS {
            return Err(ActiveValidationError::Domain(
                ValidationError::ResourceLimit("active endpoints"),
            ));
        }
        MonotonicWindow::new(started, deadline)?;
        let run_duration = deadline.elapsed_since(started)?;
        if run_duration.get() <= 0.0 || run_duration.get() > limits.overall_duration().get() {
            return Err(ActiveValidationError::Domain(ValidationError::OutOfRange(
                "active run duration",
            )));
        }
        if provenance.clock_epoch() != started.epoch {
            return Err(ActiveValidationError::Domain(
                ValidationError::ClockEpochMismatch,
            ));
        }
        let mut endpoint_ids = BTreeSet::new();
        for endpoint in &endpoints {
            if !endpoint_ids.insert(endpoint.id()) {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent("duplicate active endpoint"),
                ));
            }
            endpoint.validate_for_authorization(&authorization)?;
        }
        Ok(Self {
            schema_version: ActiveSchemaVersion::V1,
            id,
            started,
            deadline,
            endpoints,
            intervals: Vec::new(),
            authorization,
            limits,
            provenance,
        })
    }

    pub const fn id(&self) -> ActiveTestRunId {
        self.id
    }

    pub const fn started(&self) -> MonotonicTimestamp {
        self.started
    }

    pub const fn deadline(&self) -> MonotonicTimestamp {
        self.deadline
    }

    pub fn endpoints(&self) -> &[ActiveEndpoint] {
        &self.endpoints
    }

    pub fn intervals(&self) -> &[ActiveInterval] {
        &self.intervals
    }

    pub const fn authorization(&self) -> &ActiveAuthorization {
        &self.authorization
    }

    pub const fn limits(&self) -> ActiveTestLimits {
        self.limits
    }

    pub const fn provenance(&self) -> &ActiveMeasurementProvenance {
        &self.provenance
    }

    pub fn create_interval(
        &mut self,
        id: ActiveIntervalId,
        duration: Seconds,
        samples_per_endpoint: u32,
    ) -> Result<ActiveInterval, ActiveValidationError> {
        if self.intervals.iter().any(|interval| interval.id() == id) {
            return Err(ActiveValidationError::Domain(
                ValidationError::Inconsistent("duplicate active interval"),
            ));
        }
        let duration_nanos = seconds_to_nanos(duration)?;
        let run_duration = self
            .deadline
            .elapsed_since(self.started)
            .map_err(ActiveValidationError::Domain)?;
        if duration.get() <= 0.0 || duration.get() > run_duration.get() || duration_nanos == 0 {
            return Err(ActiveValidationError::Domain(ValidationError::OutOfRange(
                "active interval duration",
            )));
        }
        if samples_per_endpoint == 0 {
            return Err(ActiveValidationError::Domain(
                ValidationError::ResourceLimit("active interval samples"),
            ));
        }
        let total = samples_per_endpoint
            .checked_mul(self.endpoints.len() as u32)
            .ok_or(ActiveValidationError::Domain(
                ValidationError::ResourceLimit("active interval sample count"),
            ))?;
        let scheduled_before = self.intervals.iter().try_fold(0u32, |total, interval| {
            interval
                .samples_per_endpoint()
                .checked_mul(self.endpoints.len() as u32)
                .and_then(|samples| total.checked_add(samples))
                .ok_or(ActiveValidationError::Domain(
                    ValidationError::ResourceLimit("active run sample count"),
                ))
        })?;
        if scheduled_before
            .checked_add(total)
            .is_none_or(|scheduled| scheduled > self.limits.max_samples_total)
        {
            return Err(ActiveValidationError::Domain(
                ValidationError::ResourceLimit("active run sample count"),
            ));
        }
        let spacing_nanos = seconds_to_nanos(self.limits.minimum_spacing)?;
        let timeout_nanos = seconds_to_nanos(self.limits.per_attempt_timeout())?;
        let slots = total.saturating_sub(1) as u64;
        let required_nanos = slots
            .checked_mul(spacing_nanos)
            .and_then(|offset| offset.checked_add(timeout_nanos))
            .ok_or(ActiveValidationError::Domain(
                ValidationError::ResourceLimit("active interval rate"),
            ))?;
        if required_nanos > duration_nanos {
            return Err(ActiveValidationError::Domain(ValidationError::OutOfRange(
                "active interval rate",
            )));
        }
        let end_nanos = self.started.nanoseconds.checked_add(duration_nanos).ok_or(
            ActiveValidationError::Domain(ValidationError::OutOfRange("active interval deadline")),
        )?;
        let end = MonotonicTimestamp {
            epoch: self.started.epoch,
            nanoseconds: end_nanos,
        };
        let interval = ActiveInterval {
            schema_version: ActiveSchemaVersion::V1,
            id,
            run_id: self.id,
            window: MonotonicWindow::new(self.started, end)?,
            samples_per_endpoint,
            provenance: self.provenance.clone(),
        };
        self.intervals.push(interval.clone());
        Ok(interval)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveTestRunWire {
    schema_version: ActiveSchemaVersion,
    id: ActiveTestRunId,
    started: ActiveTimestampWire,
    deadline: ActiveTimestampWire,
    endpoints: Vec<ActiveEndpoint>,
    #[serde(default)]
    intervals: Vec<ActiveInterval>,
    authorization: ActiveAuthorization,
    limits: ActiveTestLimits,
    provenance: ActiveMeasurementProvenance,
}

impl TryFrom<ActiveTestRunWire> for ActiveTestRun {
    type Error = ActiveValidationError;

    fn try_from(value: ActiveTestRunWire) -> Result<Self, Self::Error> {
        if value.schema_version != ActiveSchemaVersion::V1 {
            return Err(ActiveValidationError::Domain(
                ValidationError::UnsupportedSchema,
            ));
        }
        let mut run = Self::new(
            value.id,
            value.started.into_timestamp(),
            value.deadline.into_timestamp(),
            value.endpoints,
            value.authorization,
            value.limits,
            value.provenance,
        )?;
        let mut interval_ids = BTreeSet::new();
        let mut scheduled_samples = 0u32;
        for interval in value.intervals {
            let interval_samples = interval
                .samples_per_endpoint()
                .checked_mul(run.endpoints.len() as u32)
                .ok_or(ActiveValidationError::Domain(
                    ValidationError::ResourceLimit("active run sample count"),
                ))?;
            if interval.run_id() != run.id
                || interval.window().start().epoch != run.started.epoch
                || interval.window().start().nanoseconds < run.started.nanoseconds
                || interval.window().end().nanoseconds > run.deadline.nanoseconds
                || interval.provenance() != &run.provenance
                || !interval_ids.insert(interval.id())
                || scheduled_samples
                    .checked_add(interval_samples)
                    .is_none_or(|total| total > run.limits.max_samples_total())
            {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent("active run interval contract"),
                ));
            }
            scheduled_samples += interval_samples;
            run.intervals.push(interval);
        }
        Ok(run)
    }
}

impl From<ActiveTestRun> for ActiveTestRunWire {
    fn from(value: ActiveTestRun) -> Self {
        Self {
            schema_version: value.schema_version,
            id: value.id,
            started: value.started.into(),
            deadline: value.deadline.into(),
            endpoints: value.endpoints,
            intervals: value.intervals,
            authorization: value.authorization,
            limits: value.limits,
            provenance: value.provenance,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ActiveIntervalWire", into = "ActiveIntervalWire")]
pub struct ActiveInterval {
    schema_version: ActiveSchemaVersion,
    id: ActiveIntervalId,
    run_id: ActiveTestRunId,
    window: MonotonicWindow,
    samples_per_endpoint: u32,
    provenance: ActiveMeasurementProvenance,
}

impl ActiveInterval {
    pub const fn id(&self) -> ActiveIntervalId {
        self.id
    }

    pub const fn run_id(&self) -> ActiveTestRunId {
        self.run_id
    }

    pub const fn window(&self) -> MonotonicWindow {
        self.window
    }

    pub const fn samples_per_endpoint(&self) -> u32 {
        self.samples_per_endpoint
    }

    pub const fn provenance(&self) -> &ActiveMeasurementProvenance {
        &self.provenance
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveIntervalWire {
    schema_version: ActiveSchemaVersion,
    id: ActiveIntervalId,
    run_id: ActiveTestRunId,
    window: ActiveWindowWire,
    samples_per_endpoint: u32,
    provenance: ActiveMeasurementProvenance,
}

impl TryFrom<ActiveIntervalWire> for ActiveInterval {
    type Error = ActiveValidationError;

    fn try_from(value: ActiveIntervalWire) -> Result<Self, Self::Error> {
        if value.schema_version != ActiveSchemaVersion::V1 {
            return Err(ActiveValidationError::Domain(
                ValidationError::UnsupportedSchema,
            ));
        }
        let window = value.window.into_window()?;
        if value.samples_per_endpoint == 0
            || value.samples_per_endpoint > MAX_ACTIVE_SAMPLES
            || window.end().nanoseconds <= window.start().nanoseconds
            || value.provenance.clock_epoch() != window.start().epoch
        {
            return Err(ActiveValidationError::Domain(
                ValidationError::Inconsistent("active interval shape"),
            ));
        }
        Ok(Self {
            schema_version: ActiveSchemaVersion::V1,
            id: value.id,
            run_id: value.run_id,
            window,
            samples_per_endpoint: value.samples_per_endpoint,
            provenance: value.provenance,
        })
    }
}

impl From<ActiveInterval> for ActiveIntervalWire {
    fn from(value: ActiveInterval) -> Self {
        Self {
            schema_version: value.schema_version,
            id: value.id,
            run_id: value.run_id,
            window: value.window.into(),
            samples_per_endpoint: value.samples_per_endpoint,
            provenance: value.provenance,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveSampleOutcome {
    Success,
    ConnectionRefused,
    Timeout,
    Unreachable,
    PermissionDenied,
    Error,
    Cancelled,
}

impl ActiveSampleOutcome {
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Success)
    }

    pub const fn is_cancelled(self) -> bool {
        matches!(self, Self::Cancelled)
    }

    pub const fn is_attempt_failure(self) -> bool {
        matches!(
            self,
            Self::ConnectionRefused
                | Self::Timeout
                | Self::Unreachable
                | Self::PermissionDenied
                | Self::Error
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ActiveSampleWire", into = "ActiveSampleWire")]
pub struct ActiveSample {
    schema_version: ActiveSchemaVersion,
    id: ActiveSampleId,
    run_id: ActiveTestRunId,
    interval_id: ActiveIntervalId,
    endpoint_id: ActiveEndpointId,
    endpoint_tier: ActiveEndpointTier,
    endpoint_attribution: EndpointAttribution,
    ordinal: u32,
    started: MonotonicTimestamp,
    finished: MonotonicTimestamp,
    outcome: ActiveSampleOutcome,
    tcp_connect_duration: Evidence<Milliseconds>,
    provenance: ActiveMeasurementProvenance,
}

impl ActiveSample {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ActiveSampleId,
        run_id: ActiveTestRunId,
        interval_id: ActiveIntervalId,
        endpoint_id: ActiveEndpointId,
        endpoint_tier: ActiveEndpointTier,
        endpoint_attribution: EndpointAttribution,
        ordinal: u32,
        started: MonotonicTimestamp,
        finished: MonotonicTimestamp,
        outcome: ActiveSampleOutcome,
        tcp_connect_duration: Evidence<Milliseconds>,
        provenance: ActiveMeasurementProvenance,
    ) -> Result<Self, ActiveValidationError> {
        finished.elapsed_since(started)?;
        if provenance.clock_epoch() != started.epoch || finished.epoch != started.epoch {
            return Err(ActiveValidationError::Domain(
                ValidationError::ClockEpochMismatch,
            ));
        }
        let elapsed_nanos = finished
            .nanoseconds
            .checked_sub(started.nanoseconds)
            .ok_or(ActiveValidationError::Domain(ValidationError::ReversedTime))?;
        match (outcome, &tcp_connect_duration) {
            (ActiveSampleOutcome::Success, Evidence::Known(duration))
                if duration.get() == elapsed_nanos as f64 / 1e6 => {}
            (ActiveSampleOutcome::Success, Evidence::Known(_)) => {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "TCP connect duration differs from monotonic sample window",
                    ),
                ));
            }
            (ActiveSampleOutcome::Success, Evidence::Unknown(_)) => {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent("successful TCP connect missing duration"),
                ));
            }
            (ActiveSampleOutcome::Cancelled, Evidence::Unknown(UnknownReason::NotMeasured)) => {}
            (ActiveSampleOutcome::Cancelled, Evidence::Unknown(_)) => {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "cancelled TCP attempt must have not-measured duration",
                    ),
                ));
            }
            (outcome, Evidence::Unknown(UnknownReason::FailedTest))
                if outcome.is_attempt_failure() => {}
            (outcome, Evidence::Unknown(_)) if outcome.is_attempt_failure() => {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "failed TCP attempt must have failed-test duration",
                    ),
                ));
            }
            (_, Evidence::Known(_)) => {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent("failed TCP attempt has duration"),
                ));
            }
            (_, Evidence::Unknown(_)) => {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "TCP attempt duration has an incompatible unknown reason",
                    ),
                ));
            }
        }
        Ok(Self {
            schema_version: ActiveSchemaVersion::V1,
            id,
            run_id,
            interval_id,
            endpoint_id,
            endpoint_tier,
            endpoint_attribution,
            ordinal,
            started,
            finished,
            outcome,
            tcp_connect_duration,
            provenance,
        })
    }

    pub const fn id(&self) -> ActiveSampleId {
        self.id
    }

    pub const fn run_id(&self) -> ActiveTestRunId {
        self.run_id
    }

    pub const fn interval_id(&self) -> ActiveIntervalId {
        self.interval_id
    }

    pub const fn endpoint_id(&self) -> ActiveEndpointId {
        self.endpoint_id
    }

    pub const fn endpoint_tier(&self) -> ActiveEndpointTier {
        self.endpoint_tier
    }

    pub const fn endpoint_attribution(&self) -> &EndpointAttribution {
        &self.endpoint_attribution
    }

    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    pub const fn started(&self) -> MonotonicTimestamp {
        self.started
    }

    pub const fn finished(&self) -> MonotonicTimestamp {
        self.finished
    }

    pub const fn outcome(&self) -> ActiveSampleOutcome {
        self.outcome
    }

    pub const fn tcp_connect_duration(&self) -> &Evidence<Milliseconds> {
        &self.tcp_connect_duration
    }

    pub const fn provenance(&self) -> &ActiveMeasurementProvenance {
        &self.provenance
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveSampleWire {
    schema_version: ActiveSchemaVersion,
    id: ActiveSampleId,
    run_id: ActiveTestRunId,
    interval_id: ActiveIntervalId,
    endpoint_id: ActiveEndpointId,
    endpoint_tier: ActiveEndpointTier,
    endpoint_attribution: EndpointAttribution,
    ordinal: u32,
    started: ActiveTimestampWire,
    finished: ActiveTimestampWire,
    outcome: ActiveSampleOutcome,
    tcp_connect_duration: Evidence<Milliseconds>,
    provenance: ActiveMeasurementProvenance,
}

impl TryFrom<ActiveSampleWire> for ActiveSample {
    type Error = ActiveValidationError;

    fn try_from(value: ActiveSampleWire) -> Result<Self, Self::Error> {
        if value.schema_version != ActiveSchemaVersion::V1 {
            return Err(ActiveValidationError::Domain(
                ValidationError::UnsupportedSchema,
            ));
        }
        Self::new(
            value.id,
            value.run_id,
            value.interval_id,
            value.endpoint_id,
            value.endpoint_tier,
            value.endpoint_attribution,
            value.ordinal,
            value.started.into_timestamp(),
            value.finished.into_timestamp(),
            value.outcome,
            value.tcp_connect_duration,
            value.provenance,
        )
    }
}

impl From<ActiveSample> for ActiveSampleWire {
    fn from(value: ActiveSample) -> Self {
        Self {
            schema_version: value.schema_version,
            id: value.id,
            run_id: value.run_id,
            interval_id: value.interval_id,
            endpoint_id: value.endpoint_id,
            endpoint_tier: value.endpoint_tier,
            endpoint_attribution: value.endpoint_attribution,
            ordinal: value.ordinal,
            started: value.started.into(),
            finished: value.finished.into(),
            outcome: value.outcome,
            tcp_connect_duration: value.tcp_connect_duration,
            provenance: value.provenance,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "ConnectTimingDistributionWire",
    into = "ConnectTimingDistributionWire"
)]
pub struct ConnectTimingDistribution {
    successful_samples: u32,
    median: Evidence<Milliseconds>,
    p90: Evidence<Milliseconds>,
    p95: Evidence<Milliseconds>,
    p99: Evidence<Milliseconds>,
    max: Evidence<Milliseconds>,
}

impl ConnectTimingDistribution {
    pub const fn successful_samples(&self) -> u32 {
        self.successful_samples
    }

    pub const fn median(&self) -> &Evidence<Milliseconds> {
        &self.median
    }

    pub const fn p90(&self) -> &Evidence<Milliseconds> {
        &self.p90
    }

    pub const fn p95(&self) -> &Evidence<Milliseconds> {
        &self.p95
    }

    pub const fn p99(&self) -> &Evidence<Milliseconds> {
        &self.p99
    }

    pub const fn max(&self) -> &Evidence<Milliseconds> {
        &self.max
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectTimingDistributionWire {
    successful_samples: u32,
    median: Evidence<Milliseconds>,
    p90: Evidence<Milliseconds>,
    p95: Evidence<Milliseconds>,
    p99: Evidence<Milliseconds>,
    max: Evidence<Milliseconds>,
}

impl TryFrom<ConnectTimingDistributionWire> for ConnectTimingDistribution {
    type Error = ActiveValidationError;

    fn try_from(value: ConnectTimingDistributionWire) -> Result<Self, Self::Error> {
        if value.successful_samples > MAX_ACTIVE_SAMPLES {
            return Err(ActiveValidationError::Domain(
                ValidationError::ResourceLimit("active connect timing samples"),
            ));
        }
        let values = [
            &value.median,
            &value.p90,
            &value.p95,
            &value.p99,
            &value.max,
        ];
        if value.successful_samples == 0 {
            if values
                .iter()
                .any(|value| matches!(value, Evidence::Known(_)))
            {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "active connect timing distribution without successes",
                    ),
                ));
            }
            let first_reason = values.first().and_then(|value| match value {
                Evidence::Unknown(reason) => Some(reason),
                Evidence::Known(_) => None,
            });
            if !matches!(
                first_reason,
                Some(UnknownReason::NotMeasured | UnknownReason::FailedTest)
            ) || values.iter().any(|value| {
                !matches!(
                    value,
                    Evidence::Unknown(reason) if Some(reason) == first_reason
                )
            }) {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "active connect timing zero-success unknown reason",
                    ),
                ));
            }
        } else {
            let mut previous = 0.0;
            for value in values {
                let Evidence::Known(value) = value else {
                    return Err(ActiveValidationError::Domain(
                        ValidationError::Inconsistent(
                            "active connect timing distribution missing percentile",
                        ),
                    ));
                };
                if value.get() < previous {
                    return Err(ActiveValidationError::Domain(
                        ValidationError::Inconsistent(
                            "active connect timing percentiles are unordered",
                        ),
                    ));
                }
                previous = value.get();
            }
            if value.successful_samples == 1
                && values
                    .windows(2)
                    .any(|pair| pair[0].as_known() != pair[1].as_known())
            {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "one-success connect timing percentiles must be identical",
                    ),
                ));
            }
        }
        Ok(Self {
            successful_samples: value.successful_samples,
            median: value.median,
            p90: value.p90,
            p95: value.p95,
            p99: value.p99,
            max: value.max,
        })
    }
}

impl From<ConnectTimingDistribution> for ConnectTimingDistributionWire {
    fn from(value: ConnectTimingDistribution) -> Self {
        Self {
            successful_samples: value.successful_samples,
            median: value.median,
            p90: value.p90,
            p95: value.p95,
            p99: value.p99,
            max: value.max,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "TcpAttemptFailureBurstDistributionWire",
    into = "TcpAttemptFailureBurstDistributionWire"
)]
pub struct TcpAttemptFailureBurstDistribution {
    burst_count: u32,
    failed_samples: u32,
    median: Evidence<u32>,
    p90: Evidence<u32>,
    p95: Evidence<u32>,
    p99: Evidence<u32>,
    max: Evidence<u32>,
}

impl TcpAttemptFailureBurstDistribution {
    pub const fn burst_count(&self) -> u32 {
        self.burst_count
    }

    pub const fn failed_samples(&self) -> u32 {
        self.failed_samples
    }

    pub const fn median(&self) -> &Evidence<u32> {
        &self.median
    }

    pub const fn p90(&self) -> &Evidence<u32> {
        &self.p90
    }

    pub const fn p95(&self) -> &Evidence<u32> {
        &self.p95
    }

    pub const fn p99(&self) -> &Evidence<u32> {
        &self.p99
    }

    pub const fn max(&self) -> &Evidence<u32> {
        &self.max
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TcpAttemptFailureBurstDistributionWire {
    burst_count: u32,
    failed_samples: u32,
    median: Evidence<u32>,
    p90: Evidence<u32>,
    p95: Evidence<u32>,
    p99: Evidence<u32>,
    max: Evidence<u32>,
}

impl TryFrom<TcpAttemptFailureBurstDistributionWire> for TcpAttemptFailureBurstDistribution {
    type Error = ActiveValidationError;

    fn try_from(value: TcpAttemptFailureBurstDistributionWire) -> Result<Self, Self::Error> {
        if value.burst_count > MAX_ACTIVE_SAMPLES || value.failed_samples > MAX_ACTIVE_SAMPLES {
            return Err(ActiveValidationError::Domain(
                ValidationError::ResourceLimit("active TCP attempt failure samples"),
            ));
        }
        let percentiles = [
            &value.median,
            &value.p90,
            &value.p95,
            &value.p99,
            &value.max,
        ];
        if value.burst_count == 0 {
            if value.failed_samples != 0
                || percentiles
                    .iter()
                    .any(|value| !matches!(value, Evidence::Unknown(UnknownReason::NotApplicable)))
            {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "active TCP attempt failure burst distribution without bursts",
                    ),
                ));
            }
        } else {
            if value.failed_samples < value.burst_count {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "active failure burst count exceeds failed attempts",
                    ),
                ));
            }
            let mut previous = 0;
            for percentile in percentiles {
                let Evidence::Known(percentile) = percentile else {
                    return Err(ActiveValidationError::Domain(
                        ValidationError::Inconsistent(
                            "active failure burst distribution missing percentile",
                        ),
                    ));
                };
                if *percentile == 0 || *percentile < previous || *percentile > value.failed_samples
                {
                    return Err(ActiveValidationError::Domain(
                        ValidationError::Inconsistent(
                            "active failure burst percentiles are invalid",
                        ),
                    ));
                }
                previous = *percentile;
            }
            let minimum_possible_max = value.failed_samples / value.burst_count
                + u32::from(!value.failed_samples.is_multiple_of(value.burst_count));
            let Evidence::Known(maximum) = &value.max else {
                unreachable!("validated failure burst maximum is known")
            };
            if *maximum < minimum_possible_max
                || (value.burst_count == 1
                    && percentiles
                        .iter()
                        .any(|value| value != &&Evidence::Known(*maximum)))
                || (value.failed_samples == value.burst_count
                    && percentiles
                        .iter()
                        .any(|value| value != &&Evidence::Known(1)))
            {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "active failure burst count and percentiles are inconsistent",
                    ),
                ));
            }
        }
        Ok(Self {
            burst_count: value.burst_count,
            failed_samples: value.failed_samples,
            median: value.median,
            p90: value.p90,
            p95: value.p95,
            p99: value.p99,
            max: value.max,
        })
    }
}

impl From<TcpAttemptFailureBurstDistribution> for TcpAttemptFailureBurstDistributionWire {
    fn from(value: TcpAttemptFailureBurstDistribution) -> Self {
        Self {
            burst_count: value.burst_count,
            failed_samples: value.failed_samples,
            median: value.median,
            p90: value.p90,
            p95: value.p95,
            p99: value.p99,
            max: value.max,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ActiveStatisticsWire", into = "ActiveStatisticsWire")]
pub struct ActiveStatistics {
    scheduled_samples: u32,
    eligible_samples: u32,
    cancelled_samples: u32,
    tcp_attempt_failure_percent: Evidence<Percentage>,
    packet_loss_percent: Evidence<Percentage>,
    connect_timing: ConnectTimingDistribution,
    tcp_attempt_failure_bursts: TcpAttemptFailureBurstDistribution,
}

impl ActiveStatistics {
    pub fn from_samples(samples: &[ActiveSample]) -> Result<Self, ActiveValidationError> {
        if samples.len() > MAX_ACTIVE_SAMPLES as usize {
            return Err(ActiveValidationError::Domain(
                ValidationError::ResourceLimit("active samples"),
            ));
        }
        let scheduled_samples = samples.len() as u32;
        let mut timings = Vec::new();
        let mut cancelled_samples = 0;
        let mut tcp_attempt_failure_bursts = Vec::new();
        let mut current_burst = 0u32;
        let mut failed_samples = 0u32;
        for sample in samples {
            match sample.outcome() {
                ActiveSampleOutcome::Success => {
                    if let Evidence::Known(timing) = sample.tcp_connect_duration() {
                        timings.push(timing.get());
                    }
                    if current_burst > 0 {
                        tcp_attempt_failure_bursts.push(current_burst);
                        current_burst = 0;
                    }
                }
                ActiveSampleOutcome::Cancelled => {
                    cancelled_samples += 1;
                    if current_burst > 0 {
                        tcp_attempt_failure_bursts.push(current_burst);
                        current_burst = 0;
                    }
                }
                outcome if outcome.is_attempt_failure() => {
                    failed_samples += 1;
                    current_burst += 1;
                }
                _ => unreachable!("all active outcomes are covered"),
            }
        }
        if current_burst > 0 {
            tcp_attempt_failure_bursts.push(current_burst);
        }
        timings.sort_by(f64::total_cmp);
        tcp_attempt_failure_bursts.sort_unstable();
        let eligible_samples = scheduled_samples.saturating_sub(cancelled_samples);
        let tcp_attempt_failure_percent = if eligible_samples == 0 {
            Evidence::Unknown(UnknownReason::NotMeasured)
        } else {
            Evidence::Known(Percentage::new(
                f64::from(failed_samples) * 100.0 / f64::from(eligible_samples),
            )?)
        };
        let no_timing_reason = if eligible_samples == 0 {
            UnknownReason::NotMeasured
        } else {
            UnknownReason::FailedTest
        };
        let no_burst_reason = if tcp_attempt_failure_bursts.is_empty() {
            UnknownReason::NotApplicable
        } else {
            no_timing_reason.clone()
        };
        Ok(Self {
            scheduled_samples,
            eligible_samples,
            cancelled_samples,
            tcp_attempt_failure_percent,
            packet_loss_percent: Evidence::Unknown(UnknownReason::NotMeasured),
            connect_timing: ConnectTimingDistribution {
                successful_samples: timings.len() as u32,
                median: percentile_millis(&timings, 0.50, no_timing_reason.clone())?,
                p90: percentile_millis(&timings, 0.90, no_timing_reason.clone())?,
                p95: percentile_millis(&timings, 0.95, no_timing_reason.clone())?,
                p99: percentile_millis(&timings, 0.99, no_timing_reason.clone())?,
                max: max_millis(&timings, no_timing_reason.clone())?,
            },
            tcp_attempt_failure_bursts: TcpAttemptFailureBurstDistribution {
                burst_count: tcp_attempt_failure_bursts.len() as u32,
                failed_samples,
                median: percentile_count(
                    &tcp_attempt_failure_bursts,
                    0.50,
                    no_burst_reason.clone(),
                ),
                p90: percentile_count(&tcp_attempt_failure_bursts, 0.90, no_burst_reason.clone()),
                p95: percentile_count(&tcp_attempt_failure_bursts, 0.95, no_burst_reason.clone()),
                p99: percentile_count(&tcp_attempt_failure_bursts, 0.99, no_burst_reason.clone()),
                max: max_count(&tcp_attempt_failure_bursts, no_burst_reason),
            },
        })
    }

    pub const fn scheduled_samples(&self) -> u32 {
        self.scheduled_samples
    }

    pub const fn eligible_samples(&self) -> u32 {
        self.eligible_samples
    }

    pub const fn cancelled_samples(&self) -> u32 {
        self.cancelled_samples
    }

    pub const fn tcp_attempt_failure_percent(&self) -> &Evidence<Percentage> {
        &self.tcp_attempt_failure_percent
    }

    pub const fn packet_loss_percent(&self) -> &Evidence<Percentage> {
        &self.packet_loss_percent
    }

    pub const fn connect_timing(&self) -> &ConnectTimingDistribution {
        &self.connect_timing
    }

    pub const fn tcp_attempt_failure_bursts(&self) -> &TcpAttemptFailureBurstDistribution {
        &self.tcp_attempt_failure_bursts
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveStatisticsWire {
    scheduled_samples: u32,
    eligible_samples: u32,
    cancelled_samples: u32,
    tcp_attempt_failure_percent: Evidence<Percentage>,
    packet_loss_percent: Evidence<Percentage>,
    connect_timing: ConnectTimingDistribution,
    tcp_attempt_failure_bursts: TcpAttemptFailureBurstDistribution,
}

impl TryFrom<ActiveStatisticsWire> for ActiveStatistics {
    type Error = ActiveValidationError;

    fn try_from(value: ActiveStatisticsWire) -> Result<Self, Self::Error> {
        if value.scheduled_samples > MAX_ACTIVE_SAMPLES {
            return Err(ActiveValidationError::Domain(
                ValidationError::ResourceLimit("active statistics samples"),
            ));
        }
        if value.cancelled_samples > value.scheduled_samples
            || value.eligible_samples
                != value
                    .scheduled_samples
                    .saturating_sub(value.cancelled_samples)
        {
            return Err(ActiveValidationError::Domain(
                ValidationError::Inconsistent("active sample eligibility counts"),
            ));
        }
        if !matches!(
            &value.packet_loss_percent,
            Evidence::Unknown(UnknownReason::NotMeasured)
        ) {
            return Err(ActiveValidationError::Domain(
                ValidationError::Inconsistent("packet loss is not measured by TCP connect timing"),
            ));
        }
        if value.eligible_samples == 0 {
            if matches!(&value.tcp_attempt_failure_percent, Evidence::Known(_))
                || value.connect_timing.successful_samples != 0
                || value.tcp_attempt_failure_bursts.failed_samples != 0
                || value.tcp_attempt_failure_bursts.burst_count != 0
            {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "active statistics contain measurements without eligible samples",
                    ),
                ));
            }
            if !matches!(
                &value.tcp_attempt_failure_percent,
                Evidence::Unknown(UnknownReason::NotMeasured)
            ) || !matches!(
                value.connect_timing.median(),
                Evidence::Unknown(UnknownReason::NotMeasured)
            ) {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "active statistics without eligible samples are not measured",
                    ),
                ));
            }
        } else {
            let Evidence::Known(tcp_attempt_failure_percent) = &value.tcp_attempt_failure_percent
            else {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent("active attempt failure percentage missing"),
                ));
            };
            let successful = value.connect_timing.successful_samples;
            let failed = value.tcp_attempt_failure_bursts.failed_samples;
            if successful
                .checked_add(failed)
                .is_none_or(|measured| measured != value.eligible_samples)
            {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent("active success and failure counts"),
                ));
            }
            if successful == 0
                && !matches!(
                    value.connect_timing.median(),
                    Evidence::Unknown(UnknownReason::FailedTest)
                )
            {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "zero-success connect timing must be failed-test unknown",
                    ),
                ));
            }
            let expected_tcp_attempt_failure_percent =
                f64::from(failed) * 100.0 / f64::from(value.eligible_samples);
            if tcp_attempt_failure_percent.get() != expected_tcp_attempt_failure_percent {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent(
                        "active attempt failure percentage does not match samples",
                    ),
                ));
            }
        }
        Ok(Self {
            scheduled_samples: value.scheduled_samples,
            eligible_samples: value.eligible_samples,
            cancelled_samples: value.cancelled_samples,
            tcp_attempt_failure_percent: value.tcp_attempt_failure_percent,
            packet_loss_percent: value.packet_loss_percent,
            connect_timing: value.connect_timing,
            tcp_attempt_failure_bursts: value.tcp_attempt_failure_bursts,
        })
    }
}

impl From<ActiveStatistics> for ActiveStatisticsWire {
    fn from(value: ActiveStatistics) -> Self {
        Self {
            scheduled_samples: value.scheduled_samples,
            eligible_samples: value.eligible_samples,
            cancelled_samples: value.cancelled_samples,
            tcp_attempt_failure_percent: value.tcp_attempt_failure_percent,
            packet_loss_percent: value.packet_loss_percent,
            connect_timing: value.connect_timing,
            tcp_attempt_failure_bursts: value.tcp_attempt_failure_bursts,
        }
    }
}

fn percentile_index(len: usize, quantile: f64) -> (usize, usize, f64) {
    let position = quantile * (len.saturating_sub(1) as f64);
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    (lower, upper, position - lower as f64)
}

fn percentile_millis(
    values: &[f64],
    quantile: f64,
    unknown: UnknownReason,
) -> Result<Evidence<Milliseconds>, ActiveValidationError> {
    if values.is_empty() {
        return Ok(Evidence::Unknown(unknown));
    }
    let (lower, upper, fraction) = percentile_index(values.len(), quantile);
    let value = values[lower] + (values[upper] - values[lower]) * fraction;
    Ok(Evidence::Known(Milliseconds::new(value)?))
}

fn max_millis(
    values: &[f64],
    unknown: UnknownReason,
) -> Result<Evidence<Milliseconds>, ActiveValidationError> {
    values
        .last()
        .copied()
        .map(Milliseconds::new)
        .transpose()
        .map(|value| {
            value
                .map(Evidence::Known)
                .unwrap_or(Evidence::Unknown(unknown))
        })
        .map_err(ActiveValidationError::Domain)
}

fn percentile_count(values: &[u32], quantile: f64, unknown: UnknownReason) -> Evidence<u32> {
    if values.is_empty() {
        return Evidence::Unknown(unknown);
    }
    let (lower, upper, fraction) = percentile_index(values.len(), quantile);
    let value = values[lower] as f64 + (values[upper] - values[lower]) as f64 * fraction;
    Evidence::Known(value.round() as u32)
}

fn max_count(values: &[u32], unknown: UnknownReason) -> Evidence<u32> {
    values
        .last()
        .copied()
        .map(Evidence::Known)
        .unwrap_or(Evidence::Unknown(unknown))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ActiveResultWire", into = "ActiveResultWire")]
pub struct ActiveResult {
    schema_version: ActiveSchemaVersion,
    id: ActiveResultId,
    run_id: ActiveTestRunId,
    interval_id: ActiveIntervalId,
    endpoint_id: ActiveEndpointId,
    endpoint_tier: ActiveEndpointTier,
    endpoint_attribution: EndpointAttribution,
    method: ActiveMeasurementMethod,
    samples: Vec<ActiveSample>,
    statistics: ActiveStatistics,
    provenance: ActiveMeasurementProvenance,
}

impl ActiveResult {
    pub fn from_samples(
        id: ActiveResultId,
        run_id: ActiveTestRunId,
        interval_id: ActiveIntervalId,
        endpoint: &ActiveEndpoint,
        mut samples: Vec<ActiveSample>,
    ) -> Result<Self, ActiveValidationError> {
        if samples.is_empty() || samples.len() > MAX_ACTIVE_SAMPLES as usize {
            return Err(ActiveValidationError::Domain(
                ValidationError::ResourceLimit("active result samples"),
            ));
        }
        samples.sort_by_key(ActiveSample::ordinal);
        let mut ordinals = BTreeSet::new();
        let mut sample_ids = BTreeSet::new();
        for sample in &samples {
            if sample.run_id() != run_id
                || sample.interval_id() != interval_id
                || sample.endpoint_id() != endpoint.id()
                || sample.endpoint_tier() != endpoint.tier()
                || sample.endpoint_attribution() != endpoint.attribution()
                || sample.provenance() != samples[0].provenance()
                || !sample_ids.insert(sample.id())
                || !ordinals.insert(sample.ordinal())
            {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent("active result sample identity"),
                ));
            }
        }
        if samples
            .iter()
            .enumerate()
            .any(|(expected, sample)| sample.ordinal() != expected as u32)
        {
            return Err(ActiveValidationError::Domain(
                ValidationError::Inconsistent("active result ordinal sequence"),
            ));
        }
        let statistics = ActiveStatistics::from_samples(&samples)?;
        let provenance = samples[0].provenance().clone();
        Ok(Self {
            schema_version: ActiveSchemaVersion::V1,
            id,
            run_id,
            interval_id,
            endpoint_id: endpoint.id(),
            endpoint_tier: endpoint.tier(),
            endpoint_attribution: endpoint.attribution().clone(),
            method: endpoint.method(),
            samples,
            statistics,
            provenance,
        })
    }

    pub const fn id(&self) -> ActiveResultId {
        self.id
    }

    pub const fn run_id(&self) -> ActiveTestRunId {
        self.run_id
    }

    pub const fn interval_id(&self) -> ActiveIntervalId {
        self.interval_id
    }

    pub const fn endpoint_id(&self) -> ActiveEndpointId {
        self.endpoint_id
    }

    pub const fn endpoint_tier(&self) -> ActiveEndpointTier {
        self.endpoint_tier
    }

    pub const fn endpoint_attribution(&self) -> &EndpointAttribution {
        &self.endpoint_attribution
    }

    pub const fn method(&self) -> ActiveMeasurementMethod {
        self.method
    }

    pub fn samples(&self) -> &[ActiveSample] {
        &self.samples
    }

    pub const fn statistics(&self) -> &ActiveStatistics {
        &self.statistics
    }

    pub const fn provenance(&self) -> &ActiveMeasurementProvenance {
        &self.provenance
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveResultWire {
    schema_version: ActiveSchemaVersion,
    id: ActiveResultId,
    run_id: ActiveTestRunId,
    interval_id: ActiveIntervalId,
    endpoint_id: ActiveEndpointId,
    endpoint_tier: ActiveEndpointTier,
    endpoint_attribution: EndpointAttribution,
    method: ActiveMeasurementMethod,
    samples: Vec<ActiveSample>,
    statistics: ActiveStatistics,
    provenance: ActiveMeasurementProvenance,
}

impl TryFrom<ActiveResultWire> for ActiveResult {
    type Error = ActiveValidationError;

    fn try_from(mut value: ActiveResultWire) -> Result<Self, Self::Error> {
        if value.schema_version != ActiveSchemaVersion::V1
            || value.method != ActiveMeasurementMethod::TcpConnectTiming
            || value.samples.is_empty()
            || value.samples.len() > MAX_ACTIVE_SAMPLES as usize
        {
            return Err(ActiveValidationError::Domain(
                ValidationError::Inconsistent("active result shape"),
            ));
        }
        value.samples.sort_by_key(ActiveSample::ordinal);
        let mut sample_ids = BTreeSet::new();
        for (expected, sample) in value.samples.iter().enumerate() {
            if sample.run_id() != value.run_id
                || sample.interval_id() != value.interval_id
                || sample.endpoint_id() != value.endpoint_id
                || sample.endpoint_tier() != value.endpoint_tier
                || sample.endpoint_attribution() != &value.endpoint_attribution
                || sample.provenance() != &value.provenance
                || !sample_ids.insert(sample.id())
                || sample.ordinal() != expected as u32
            {
                return Err(ActiveValidationError::Domain(
                    ValidationError::Inconsistent("active result sample identity"),
                ));
            }
        }
        let expected_statistics = ActiveStatistics::from_samples(&value.samples)?;
        if expected_statistics != value.statistics {
            return Err(ActiveValidationError::Domain(
                ValidationError::Inconsistent("active result statistics"),
            ));
        }
        Ok(Self {
            schema_version: ActiveSchemaVersion::V1,
            id: value.id,
            run_id: value.run_id,
            interval_id: value.interval_id,
            endpoint_id: value.endpoint_id,
            endpoint_tier: value.endpoint_tier,
            endpoint_attribution: value.endpoint_attribution,
            method: value.method,
            samples: value.samples,
            statistics: value.statistics,
            provenance: value.provenance,
        })
    }
}

impl From<ActiveResult> for ActiveResultWire {
    fn from(value: ActiveResult) -> Self {
        Self {
            schema_version: value.schema_version,
            id: value.id,
            run_id: value.run_id,
            interval_id: value.interval_id,
            endpoint_id: value.endpoint_id,
            endpoint_tier: value.endpoint_tier,
            endpoint_attribution: value.endpoint_attribution,
            method: value.method,
            samples: value.samples,
            statistics: value.statistics,
            provenance: value.provenance,
        }
    }
}
