//! Bounded, read-only Kismet REST status and capability adapter.
//!
//! This module deliberately stops at operational status.  It does not call
//! capture helpers, write Kismet state, or turn datasource/device aggregates
//! into packet observations.  Kismet JSON is decoded into Kyberia-owned types
//! only after an authenticated response has passed bounded transport and
//! strict JSON checks.

use serde::Serialize;
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const LIVE_STATUS_SCHEMA: &str = "kyberia.kismet-live-status/1";
const STATUS_PATH: &str = "/system/status.json";
const TYPES_PATH: &str = "/datasource/types.json";
const SOURCES_PATH: &str = "/datasource/all_sources.json";
const MAX_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(60);
const DUPLICATE_KEY_ERROR: &str = "duplicate JSON object key";
// The pinned Kismet source generates date versions as YYYY.MM.0 by default
// (tools/mkversion.sh), so the date alone cannot establish API compatibility.
// This initial status contract admits only the inspected acceptance fixture
// and source identity; expanding it requires a new pinned server review.
const SUPPORTED_KISMET_VERSION: (u16, u16, u16) = (2026, 9, 0);
const SUPPORTED_KISMET_GIT_SHORT: &str = "2d25ad0";
const SUPPORTED_KISMET_GIT_FULL: &str = "2d25ad004e9216ac963c4f156e9077331717959c";

/// A secret API key.  It is intentionally neither serializable nor
/// debuggable; callers should keep it in a secret store and pass it only to
/// [`KismetLiveClient::connect`].
pub struct ApiToken(String);

impl ApiToken {
    pub fn new(token: impl Into<String>) -> Result<Self, LiveError> {
        let token = token.into();
        if token.is_empty()
            || token.len() > 4096
            || token
                .bytes()
                .any(|byte| !byte.is_ascii_graphic() || matches!(byte, b';' | b','))
        {
            return Err(LiveError::InvalidToken);
        }
        Ok(Self(token))
    }

    fn cookie_value(&self) -> String {
        format!("KISMET={}", self.0)
    }
}

impl fmt::Debug for ApiToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiToken(<redacted>)")
    }
}

/// A Kismet base URL.  Query strings, fragments and userinfo are rejected so
/// the token can never be moved into an attacker-controlled URL component.
#[derive(Clone, PartialEq, Eq)]
pub struct Endpoint(String);

impl Endpoint {
    pub fn new(base: impl Into<String>) -> Result<Self, LiveError> {
        let mut base = base.into();
        if base.len() > 4096
            || base.bytes().any(|byte| byte.is_ascii_control())
            || !(base.starts_with("http://") || base.starts_with("https://"))
        {
            return Err(LiveError::InvalidEndpoint);
        }
        if base.contains('?') || base.contains('#') {
            return Err(LiveError::InvalidEndpoint);
        }
        let authority = base
            .split_once("://")
            .and_then(|(_, remainder)| remainder.split('/').next())
            .unwrap_or_default();
        if authority.is_empty()
            || authority.contains('@')
            || authority
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte == b'\\')
        {
            return Err(LiveError::InvalidEndpoint);
        }
        while base.ends_with('/') {
            base.pop();
        }
        if base.is_empty() {
            return Err(LiveError::InvalidEndpoint);
        }
        Ok(Self(base))
    }

    fn url_for(&self, path: &str) -> String {
        format!("{}{}", self.0, path)
    }
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Endpoint(<redacted>)")
    }
}

/// Cooperative cancellation for polling between bounded HTTP operations and
/// retry backoff.  A blocking socket read is bounded by the configured ureq
/// request timeout; cancellation cannot interrupt a syscall already in
/// progress.
#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl fmt::Debug for CancellationToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveLimits {
    pub max_body_bytes: usize,
    pub max_json_depth: usize,
    pub max_datasource_types: usize,
    pub max_datasources: usize,
    pub max_list_items: usize,
    pub max_string_bytes: usize,
    pub request_timeout: Duration,
    pub max_retries: u8,
    pub retry_backoff: Duration,
}

impl Default for LiveLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 1024 * 1024,
            max_json_depth: 32,
            max_datasource_types: 256,
            max_datasources: 1024,
            max_list_items: 4096,
            max_string_bytes: 4096,
            request_timeout: Duration::from_secs(5),
            max_retries: 2,
            retry_backoff: Duration::from_millis(100),
        }
    }
}

impl LiveLimits {
    fn validate(self) -> Result<Self, LiveError> {
        if !(1..=16 * 1024 * 1024).contains(&self.max_body_bytes)
            || !(1..=64).contains(&self.max_json_depth)
            || !(1..=4096).contains(&self.max_datasource_types)
            || !(1..=16_384).contains(&self.max_datasources)
            || !(1..=65_536).contains(&self.max_list_items)
            || !(1..=64 * 1024).contains(&self.max_string_bytes)
            || self.request_timeout.is_zero()
            || self.request_timeout > MAX_ATTEMPT_TIMEOUT
            || self.max_retries > 5
            || self.retry_backoff > Duration::from_secs(10)
        {
            return Err(LiveError::InvalidLimits);
        }
        Ok(self)
    }

    fn attempt_timeout(self) -> Duration {
        let attempts = u32::from(self.max_retries) + 1;
        let nanos = self.request_timeout.as_nanos() / u128::from(attempts);
        Duration::from_nanos(nanos.max(1) as u64)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveError {
    InvalidEndpoint,
    InvalidToken,
    InvalidLimits,
    Cancelled,
    DeadlineExceeded,
    AuthenticationRequired,
    Forbidden,
    Redirect,
    HttpStatus(u16),
    Transport,
    BodyTooLarge,
    TruncatedBody,
    MalformedJson,
    DuplicateJsonKey,
    UnsupportedVersion,
    UnsupportedSchema,
    InventoryLimit,
    SchemaViolation,
    RetryExhausted,
}

impl fmt::Display for LiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidEndpoint => "invalid Kismet endpoint",
            Self::InvalidToken => "invalid Kismet API token",
            Self::InvalidLimits => "invalid live polling limits",
            Self::Cancelled => "Kismet polling cancelled",
            Self::DeadlineExceeded => "Kismet polling deadline exceeded",
            Self::AuthenticationRequired => "Kismet authentication required",
            Self::Forbidden => "Kismet access forbidden",
            Self::Redirect => "Kismet endpoint returned a redirect",
            Self::HttpStatus(_) => "Kismet endpoint returned an unexpected status",
            Self::Transport => "Kismet transport failure",
            Self::BodyTooLarge => "Kismet response exceeds the body limit",
            Self::TruncatedBody => "Kismet response body was truncated",
            Self::MalformedJson => "Kismet response was malformed JSON",
            Self::DuplicateJsonKey => "Kismet response contained a duplicate JSON key",
            Self::UnsupportedVersion => "Kismet producer version is unsupported",
            Self::UnsupportedSchema => "Kismet response schema is unsupported",
            Self::InventoryLimit => "Kismet datasource inventory exceeds the limit",
            Self::SchemaViolation => "Kismet response violated the supported field schema",
            Self::RetryExhausted => "Kismet retry budget exhausted",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for LiveError {}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProducerStatus {
    version: ProducerVersion,
    server_name: Option<String>,
    server_description: Option<String>,
    server_location: Option<String>,
    git_revision: Option<String>,
    build_time: Option<String>,
    device_count: Option<u64>,
    clock_seconds: Option<i64>,
    clock_microseconds: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProducerVersion {
    raw: String,
    major: u16,
    minor: u16,
    patch: u16,
    build: Option<String>,
}

impl ProducerStatus {
    pub fn version(&self) -> &ProducerVersion {
        &self.version
    }

    pub fn clock_seconds(&self) -> Option<i64> {
        self.clock_seconds
    }

    pub fn clock_microseconds(&self) -> Option<i64> {
        self.clock_microseconds
    }
}

impl ProducerVersion {
    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn components(&self) -> (u16, u16, u16) {
        (self.major, self.minor, self.patch)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DatasourceType {
    source_type: String,
    description: Option<String>,
    probe_capable: Option<bool>,
    list_capable: Option<bool>,
    local_capable: Option<bool>,
    remote_capable: Option<bool>,
    passive_capable: Option<bool>,
    tune_capable: Option<bool>,
    hop_capable: Option<bool>,
}

impl DatasourceType {
    pub fn source_type(&self) -> &str {
        &self.source_type
    }

    pub fn remote_capable(&self) -> Option<bool> {
        self.remote_capable
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DatasourceStatus {
    uuid: String,
    source_type: Option<String>,
    name: Option<String>,
    interface: Option<String>,
    capture_interface: Option<String>,
    hardware: Option<String>,
    source_version: Option<String>,
    warning: Option<String>,
    error_reason: Option<String>,
    running: Option<bool>,
    paused: Option<bool>,
    remote: Option<bool>,
    passive: Option<bool>,
    hopping: Option<bool>,
    error: Option<bool>,
    channel: Option<String>,
    channels: Option<Vec<String>>,
    hop_rate: Option<f64>,
    hop_channels: Option<Vec<String>>,
    packet_count: Option<u64>,
    error_packet_count: Option<u64>,
}

impl DatasourceStatus {
    pub fn uuid(&self) -> &str {
        &self.uuid
    }

    pub fn channel(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    pub fn remote(&self) -> Option<bool> {
        self.remote
    }

    pub fn hopping(&self) -> Option<bool> {
        self.hopping
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FieldAvailability {
    status: Vec<String>,
    datasource_type: Vec<String>,
    datasource: Vec<String>,
}

impl FieldAvailability {
    pub fn status(&self) -> &[String] {
        &self.status
    }

    pub fn datasource(&self) -> &[String] {
        &self.datasource
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LiveStatusSnapshot {
    schema: String,
    adapter_version: String,
    producer: ProducerStatus,
    datasource_types: Vec<DatasourceType>,
    datasources: Vec<DatasourceStatus>,
    fields: FieldAvailability,
}

impl LiveStatusSnapshot {
    pub fn producer(&self) -> &ProducerStatus {
        &self.producer
    }

    pub fn datasource_types(&self) -> &[DatasourceType] {
        &self.datasource_types
    }

    pub fn datasources(&self) -> &[DatasourceStatus] {
        &self.datasources
    }

    pub fn fields(&self) -> &FieldAvailability {
        &self.fields
    }

    /// Stable bytes for provenance and replay.  The decoder sorts all
    /// inventory records and field names before this method is available.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("LiveStatusSnapshot contains only serializable fields")
    }

    pub fn canonical_sha256(&self) -> [u8; 32] {
        let digest = Sha256::digest(self.canonical_bytes());
        digest.into()
    }
}

/// Poll a read-only status snapshot.  Kismet's API token is sent in the
/// `KISMET` cookie on each requested resource; no session-check endpoint is
/// used because that endpoint validates browser login sessions rather than
/// API keys.
pub struct KismetLiveClient {
    endpoint: Endpoint,
    token: ApiToken,
    limits: LiveLimits,
    agent: ureq::Agent,
}

impl fmt::Debug for KismetLiveClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KismetLiveClient")
            .field("endpoint", &self.endpoint)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl KismetLiveClient {
    pub fn connect(
        endpoint: Endpoint,
        token: ApiToken,
        limits: LiveLimits,
    ) -> Result<Self, LiveError> {
        let limits = limits.validate()?;
        let attempt_timeout = limits.attempt_timeout();
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(attempt_timeout)
            .build();
        Ok(Self {
            endpoint,
            token,
            limits,
            agent,
        })
    }

    pub fn poll(&self, cancellation: &CancellationToken) -> Result<LiveStatusSnapshot, LiveError> {
        let mut clock = WallClock::new();
        self.poll_with_clock(cancellation, &mut clock)
    }

    fn poll_with_clock<C: PollClock>(
        &self,
        cancellation: &CancellationToken,
        clock: &mut C,
    ) -> Result<LiveStatusSnapshot, LiveError> {
        let executor = UreqHttp { agent: &self.agent };
        self.poll_with_executor(&executor, cancellation, clock)
    }

    fn poll_with_executor<E: HttpGet, C: PollClock>(
        &self,
        executor: &E,
        cancellation: &CancellationToken,
        clock: &mut C,
    ) -> Result<LiveStatusSnapshot, LiveError> {
        let deadline = self.limits.request_timeout;
        let status = self.request_json(executor, STATUS_PATH, cancellation, clock, deadline)?;
        let (producer, status_fields) = decode_status(&status, &self.limits)?;
        check_budget(cancellation, clock, deadline)?;
        let types = self.request_json(executor, TYPES_PATH, cancellation, clock, deadline)?;
        let (datasource_types, type_fields) = decode_types(&types, &self.limits)?;
        check_budget(cancellation, clock, deadline)?;
        let sources = self.request_json(executor, SOURCES_PATH, cancellation, clock, deadline)?;
        let (datasources, source_fields) = decode_sources(&sources, &self.limits)?;
        check_budget(cancellation, clock, deadline)?;

        Ok(LiveStatusSnapshot {
            schema: LIVE_STATUS_SCHEMA.to_owned(),
            adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
            producer,
            datasource_types,
            datasources,
            fields: FieldAvailability {
                status: status_fields,
                datasource_type: type_fields,
                datasource: source_fields,
            },
        })
    }

    fn request_json<E: HttpGet, C: PollClock>(
        &self,
        executor: &E,
        path: &str,
        cancellation: &CancellationToken,
        clock: &mut C,
        deadline: Duration,
    ) -> Result<Value, LiveError> {
        let cookie = self.token.cookie_value();
        let mut last_transient = false;
        for attempt in 0..=self.limits.max_retries {
            check_budget(cancellation, clock, deadline)?;
            let remaining = deadline
                .saturating_sub(clock.elapsed())
                .min(self.limits.attempt_timeout());
            let response = match executor.get(
                &self.endpoint,
                &self.token,
                path,
                &cookie,
                &self.limits,
                remaining,
            ) {
                Ok(response) => response,
                Err(LiveError::Transport) if attempt < self.limits.max_retries => {
                    check_budget(cancellation, clock, deadline)?;
                    last_transient = true;
                    wait_for_retry(
                        self.limits.retry_backoff,
                        attempt,
                        deadline,
                        clock,
                        cancellation,
                    )?;
                    continue;
                }
                Err(LiveError::Transport) => {
                    check_budget(cancellation, clock, deadline)?;
                    return Err(LiveError::RetryExhausted);
                }
                Err(error) => {
                    check_budget(cancellation, clock, deadline)?;
                    return Err(error);
                }
            };
            check_budget(cancellation, clock, deadline)?;
            let status = response.status;
            if (200..300).contains(&status) {
                let value = parse_json(&response.body, self.limits.max_json_depth)?;
                check_budget(cancellation, clock, deadline)?;
                return Ok(value);
            }
            if status == 401 {
                return Err(LiveError::AuthenticationRequired);
            }
            if status == 403 {
                return Err(LiveError::Forbidden);
            }
            if (300..400).contains(&status) {
                return Err(LiveError::Redirect);
            }
            if is_transient_status(status) {
                last_transient = true;
                if attempt == self.limits.max_retries {
                    break;
                }
                wait_for_retry(
                    self.limits.retry_backoff,
                    attempt,
                    deadline,
                    clock,
                    cancellation,
                )?;
                continue;
            }
            return Err(LiveError::HttpStatus(status));
        }
        if last_transient {
            Err(LiveError::RetryExhausted)
        } else {
            Err(LiveError::Transport)
        }
    }
}

struct RawResponse {
    status: u16,
    body: Vec<u8>,
}

trait HttpGet {
    fn get(
        &self,
        endpoint: &Endpoint,
        token: &ApiToken,
        path: &str,
        cookie: &str,
        limits: &LiveLimits,
        remaining: Duration,
    ) -> Result<RawResponse, LiveError>;
}

struct UreqHttp<'a> {
    agent: &'a ureq::Agent,
}

impl HttpGet for UreqHttp<'_> {
    fn get(
        &self,
        endpoint: &Endpoint,
        _token: &ApiToken,
        path: &str,
        cookie: &str,
        limits: &LiveLimits,
        remaining: Duration,
    ) -> Result<RawResponse, LiveError> {
        let url = endpoint.url_for(path);
        let response = match self
            .agent
            .get(&url)
            .timeout(remaining)
            .set("Accept", "application/json")
            .set("Cookie", cookie)
            .call()
        {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => response,
            Err(ureq::Error::Transport(transport)) => {
                if transport.kind() == ureq::ErrorKind::TooManyRedirects {
                    return Err(LiveError::Redirect);
                }
                return Err(LiveError::Transport);
            }
        };
        let status = response.status();
        if !(200..300).contains(&status) {
            // Status/error bodies are deliberately discarded.  They cannot
            // affect admission and may contain untrusted or secret-bearing
            // server diagnostics; dropping the response also prevents a
            // large 401/403/5xx body from masking its structured status.
            return Ok(RawResponse {
                status,
                body: Vec::new(),
            });
        }
        let body = read_response_body(response, limits)?;
        Ok(RawResponse { status, body })
    }
}

trait PollClock {
    fn elapsed(&self) -> Duration;
    fn sleep(
        &mut self,
        duration: Duration,
        cancellation: &CancellationToken,
    ) -> Result<(), LiveError>;
}

struct WallClock {
    started: Instant,
}

impl WallClock {
    fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl PollClock for WallClock {
    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    fn sleep(
        &mut self,
        duration: Duration,
        cancellation: &CancellationToken,
    ) -> Result<(), LiveError> {
        let slice = Duration::from_millis(10);
        let mut remaining = duration;
        while !remaining.is_zero() {
            check_cancelled(cancellation)?;
            let current = remaining.min(slice);
            std::thread::sleep(current);
            remaining = remaining.saturating_sub(current);
        }
        Ok(())
    }
}

fn retry_delay(base: Duration, attempt: u8) -> Duration {
    let multiplier = 1u32.checked_shl(u32::from(attempt)).unwrap_or(u32::MAX);
    base.checked_mul(multiplier).unwrap_or(Duration::MAX)
}

fn wait_for_retry<C: PollClock>(
    base: Duration,
    attempt: u8,
    deadline: Duration,
    clock: &mut C,
    cancellation: &CancellationToken,
) -> Result<(), LiveError> {
    let remaining = deadline.saturating_sub(clock.elapsed());
    if remaining.is_zero() {
        return Err(LiveError::DeadlineExceeded);
    }
    clock.sleep(retry_delay(base, attempt).min(remaining), cancellation)
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), LiveError> {
    if cancellation.is_cancelled() {
        Err(LiveError::Cancelled)
    } else {
        Ok(())
    }
}

fn check_budget<C: PollClock>(
    cancellation: &CancellationToken,
    clock: &C,
    deadline: Duration,
) -> Result<(), LiveError> {
    check_cancelled(cancellation)?;
    if clock.elapsed() >= deadline {
        Err(LiveError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn is_transient_status(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500..=599)
}

fn read_response_body(response: ureq::Response, limits: &LiveLimits) -> Result<Vec<u8>, LiveError> {
    let declared_length = response
        .header("Content-Length")
        .map(|value| value.parse::<usize>().map_err(|_| LiveError::MalformedJson))
        .transpose()?;
    if declared_length.is_some_and(|length| length > limits.max_body_bytes) {
        return Err(LiveError::BodyTooLarge);
    }
    let mut body = Vec::new();
    let mut reader = response
        .into_reader()
        .take(limits.max_body_bytes as u64 + 1);
    if let Err(error) = reader.read_to_end(&mut body) {
        if declared_length.is_some_and(|length| body.len() < length) {
            return Err(LiveError::TruncatedBody);
        }
        return Err(map_io_error(&error));
    }
    if body.len() > limits.max_body_bytes {
        return Err(LiveError::BodyTooLarge);
    }
    if declared_length.is_some_and(|length| length != body.len()) {
        return Err(LiveError::TruncatedBody);
    }
    Ok(body)
}

fn map_io_error(error: &io::Error) -> LiveError {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        LiveError::DeadlineExceeded
    } else {
        LiveError::Transport
    }
}

fn parse_json(bytes: &[u8], max_depth: usize) -> Result<Value, LiveError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = UniqueJsonValue::deserialize(&mut deserializer).map_err(|error| {
        if error.to_string().starts_with(DUPLICATE_KEY_ERROR) {
            LiveError::DuplicateJsonKey
        } else {
            LiveError::MalformedJson
        }
    })?;
    deserializer.end().map_err(|_| LiveError::MalformedJson)?;
    if json_depth(&value.0, 0) > max_depth {
        return Err(LiveError::UnsupportedSchema);
    }
    Ok(value.0)
}

fn json_depth(value: &Value, depth: usize) -> usize {
    match value {
        Value::Array(values) => values
            .iter()
            .map(|value| json_depth(value, depth + 1))
            .max()
            .unwrap_or(depth),
        Value::Object(values) => values
            .values()
            .map(|value| json_depth(value, depth + 1))
            .max()
            .unwrap_or(depth),
        _ => depth,
    }
}

struct UniqueJsonValue(Value);

impl<'de> Deserialize<'de> for UniqueJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UniqueVisitor;

        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueJsonValue;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON value with unique object keys")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJsonValue(Value::Bool(value)))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJsonValue(Value::Number(value.into())))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJsonValue(Value::Number(value.into())))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                serde_json::Number::from_f64(value)
                    .map(|number| UniqueJsonValue(Value::Number(number)))
                    .ok_or_else(|| de::Error::custom("non-finite JSON number"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJsonValue(Value::String(value.to_owned())))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJsonValue(Value::String(value)))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJsonValue(Value::Null))
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJsonValue(Value::Null))
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<UniqueJsonValue>()? {
                    values.push(value.0);
                }
                Ok(UniqueJsonValue(Value::Array(values)))
            }

            fn visit_map<A>(self, mut map_access: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = Map::new();
                while let Some(key) = map_access.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom(DUPLICATE_KEY_ERROR));
                    }
                    let value = map_access.next_value::<UniqueJsonValue>()?;
                    values.insert(key, value.0);
                }
                Ok(UniqueJsonValue(Value::Object(values)))
            }
        }

        deserializer.deserialize_any(UniqueVisitor)
    }
}

fn as_object(value: &Value) -> Result<&Map<String, Value>, LiveError> {
    value.as_object().ok_or(LiveError::UnsupportedSchema)
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    limits: &LiveLimits,
) -> Result<&'a str, LiveError> {
    let value = object.get(key).ok_or(LiveError::SchemaViolation)?;
    let value = value.as_str().ok_or(LiveError::SchemaViolation)?;
    if value.is_empty() || value.len() > limits.max_string_bytes {
        return Err(LiveError::SchemaViolation);
    }
    Ok(value)
}

fn optional_string(
    object: &Map<String, Value>,
    key: &str,
    limits: &LiveLimits,
) -> Result<Option<String>, LiveError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let value = value.as_str().ok_or(LiveError::SchemaViolation)?;
            if value.len() > limits.max_string_bytes {
                return Err(LiveError::SchemaViolation);
            }
            Ok(Some(value.to_owned()))
        }
    }
}

fn optional_bool(object: &Map<String, Value>, key: &str) -> Result<Option<bool>, LiveError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(Value::Number(value)) => match value.as_u64() {
            Some(0) => Ok(Some(false)),
            Some(1) => Ok(Some(true)),
            _ => Err(LiveError::SchemaViolation),
        },
        Some(_) => Err(LiveError::SchemaViolation),
    }
}

fn optional_i64(object: &Map<String, Value>, key: &str) -> Result<Option<i64>, LiveError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value.as_i64().map(Some).ok_or(LiveError::SchemaViolation),
        Some(_) => Err(LiveError::SchemaViolation),
    }
}

fn optional_timestamp_usecs(object: &Map<String, Value>) -> Result<Option<i64>, LiveError> {
    match object.get("kismet.system.timestamp.usec") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value
            .as_i64()
            .filter(|value| (0..1_000_000).contains(value))
            .map(Some)
            .ok_or(LiveError::SchemaViolation),
        Some(_) => Err(LiveError::SchemaViolation),
    }
}

fn optional_u64(object: &Map<String, Value>, key: &str) -> Result<Option<u64>, LiveError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value
            .as_u64()
            .filter(|value| *value <= 1_000_000_000_000_000)
            .map(Some)
            .ok_or(LiveError::SchemaViolation),
        Some(_) => Err(LiveError::SchemaViolation),
    }
}

fn optional_f64(object: &Map<String, Value>, key: &str) -> Result<Option<f64>, LiveError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value
            .as_f64()
            .filter(|value| value.is_finite() && (0.0..=1_000_000.0).contains(value))
            .map(Some)
            .ok_or(LiveError::SchemaViolation),
        Some(_) => Err(LiveError::SchemaViolation),
    }
}

fn optional_string_list(
    object: &Map<String, Value>,
    key: &str,
    limits: &LiveLimits,
) -> Result<Option<Vec<String>>, LiveError> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let values = value.as_array().ok_or(LiveError::SchemaViolation)?;
    if values.len() > limits.max_list_items {
        return Err(LiveError::InventoryLimit);
    }
    let mut output = Vec::with_capacity(values.len());
    for value in values {
        let value = value.as_str().ok_or(LiveError::SchemaViolation)?;
        if value.len() > limits.max_string_bytes {
            return Err(LiveError::SchemaViolation);
        }
        output.push(value.to_owned());
    }
    Ok(Some(output))
}

fn parse_producer_version(raw: &str, limits: &LiveLimits) -> Result<ProducerVersion, LiveError> {
    if raw.len() > limits.max_string_bytes {
        return Err(LiveError::UnsupportedVersion);
    }
    let (numeric, build) = match raw.split_once('-') {
        None => (raw, None),
        Some((numeric, build)) if !build.is_empty() => (numeric, Some(build.to_owned())),
        Some(_) => return Err(LiveError::UnsupportedVersion),
    };
    let mut components = numeric.split('.');
    let major = components
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or(LiveError::UnsupportedVersion)?;
    let minor = components
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or(LiveError::UnsupportedVersion)?;
    let patch = components
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or(LiveError::UnsupportedVersion)?;
    if components.next().is_some() || (major, minor, patch) != SUPPORTED_KISMET_VERSION {
        return Err(LiveError::UnsupportedVersion);
    }
    if build
        .as_ref()
        .is_some_and(|value| value.len() > limits.max_string_bytes)
    {
        return Err(LiveError::UnsupportedVersion);
    }
    Ok(ProducerVersion {
        raw: raw.to_owned(),
        major,
        minor,
        patch,
        build,
    })
}

fn decode_status(
    value: &Value,
    limits: &LiveLimits,
) -> Result<(ProducerStatus, Vec<String>), LiveError> {
    let object = as_object(value)?;
    let raw_version = required_string(object, "kismet.system.version", limits)?;
    let version = parse_producer_version(raw_version, limits)?;
    let git_revision = optional_string(object, "kismet.system.git", limits)?;
    if !git_revision.as_deref().is_some_and(|revision| {
        revision == SUPPORTED_KISMET_GIT_SHORT || revision == SUPPORTED_KISMET_GIT_FULL
    }) {
        return Err(LiveError::UnsupportedVersion);
    }
    let status_keys = [
        "kismet.system.version",
        "kismet.system.git",
        "kismet.system.build_time",
        "kismet.system.server_name",
        "kismet.system.server_description",
        "kismet.system.server_location",
        "kismet.system.devices.count",
        "kismet.system.timestamp.sec",
        "kismet.system.timestamp.usec",
    ];
    let mut fields = present_fields(object, &status_keys);
    fields.sort();
    Ok((
        ProducerStatus {
            version,
            server_name: optional_string(object, "kismet.system.server_name", limits)?,
            server_description: optional_string(
                object,
                "kismet.system.server_description",
                limits,
            )?,
            server_location: optional_string(object, "kismet.system.server_location", limits)?,
            git_revision,
            build_time: optional_string(object, "kismet.system.build_time", limits)?,
            device_count: optional_u64(object, "kismet.system.devices.count")?,
            clock_seconds: optional_i64(object, "kismet.system.timestamp.sec")?,
            clock_microseconds: optional_timestamp_usecs(object)?,
        },
        fields,
    ))
}

fn decode_types(
    value: &Value,
    limits: &LiveLimits,
) -> Result<(Vec<DatasourceType>, Vec<String>), LiveError> {
    let values = value.as_array().ok_or(LiveError::UnsupportedSchema)?;
    if values.len() > limits.max_datasource_types {
        return Err(LiveError::InventoryLimit);
    }
    let known_keys = [
        "kismet.datasource.driver.type",
        "kismet.datasource.driver.description",
        "kismet.datasource.driver.probe_capable",
        "kismet.datasource.driver.list_capable",
        "kismet.datasource.driver.local_capable",
        "kismet.datasource.driver.remote_capable",
        "kismet.datasource.driver.passive_capable",
        "kismet.datasource.driver.tuning_capable",
        "kismet.datasource_driver.hop_capable",
    ];
    let mut fields = Vec::new();
    let mut output = Vec::with_capacity(values.len());
    for value in values {
        let object = as_object(value)?;
        fields.extend(present_fields(object, &known_keys));
        output.push(DatasourceType {
            source_type: required_string(object, known_keys[0], limits)?.to_owned(),
            description: optional_string(object, known_keys[1], limits)?,
            probe_capable: optional_bool(object, known_keys[2])?,
            list_capable: optional_bool(object, known_keys[3])?,
            local_capable: optional_bool(object, known_keys[4])?,
            remote_capable: optional_bool(object, known_keys[5])?,
            passive_capable: optional_bool(object, known_keys[6])?,
            tune_capable: optional_bool(object, known_keys[7])?,
            hop_capable: optional_bool(object, known_keys[8])?,
        });
    }
    output.sort_by(|left, right| left.source_type.cmp(&right.source_type));
    if output
        .windows(2)
        .any(|window| window[0].source_type == window[1].source_type)
    {
        return Err(LiveError::SchemaViolation);
    }
    fields.sort();
    fields.dedup();
    Ok((output, fields))
}

fn decode_sources(
    value: &Value,
    limits: &LiveLimits,
) -> Result<(Vec<DatasourceStatus>, Vec<String>), LiveError> {
    let values = value.as_array().ok_or(LiveError::UnsupportedSchema)?;
    if values.len() > limits.max_datasources {
        return Err(LiveError::InventoryLimit);
    }
    let known_keys = [
        "kismet.datasource.uuid",
        "kismet.datasource.type",
        "kismet.datasource.name",
        "kismet.datasource.interface",
        "kismet.datasource.capture_interface",
        "kismet.datasource.hardware",
        "kismet.datasource.datasource_version",
        "kismet.datasource.warning",
        "kismet.datasource.error_reason",
        "kismet.datasource.running",
        "kismet.datasource.paused",
        "kismet.datasource.remote",
        "kismet.datasource.passive",
        "kismet.datasource.hopping",
        "kismet.datasource.error",
        "kismet.datasource.channel",
        "kismet.datasource.channels",
        "kismet.datasource.hop_rate",
        "kismet.datasource.hop_channels",
        "kismet.datasource.num_packets",
        "kismet.datasource.num_error_packets",
    ];
    let mut fields = Vec::new();
    let mut output = Vec::with_capacity(values.len());
    for value in values {
        let object = as_object(value)?;
        fields.extend(present_fields(object, &known_keys));
        output.push(DatasourceStatus {
            uuid: required_string(object, known_keys[0], limits)?.to_owned(),
            source_type: optional_string(object, known_keys[1], limits)?,
            name: optional_string(object, known_keys[2], limits)?,
            interface: optional_string(object, known_keys[3], limits)?,
            capture_interface: optional_string(object, known_keys[4], limits)?,
            hardware: optional_string(object, known_keys[5], limits)?,
            source_version: optional_string(object, known_keys[6], limits)?,
            warning: optional_string(object, known_keys[7], limits)?,
            error_reason: optional_string(object, known_keys[8], limits)?,
            running: optional_bool(object, known_keys[9])?,
            paused: optional_bool(object, known_keys[10])?,
            remote: optional_bool(object, known_keys[11])?,
            passive: optional_bool(object, known_keys[12])?,
            hopping: optional_bool(object, known_keys[13])?,
            error: optional_bool(object, known_keys[14])?,
            channel: optional_string(object, known_keys[15], limits)?,
            channels: optional_string_list(object, known_keys[16], limits)?,
            hop_rate: optional_f64(object, known_keys[17])?,
            hop_channels: optional_string_list(object, known_keys[18], limits)?,
            packet_count: optional_u64(object, known_keys[19])?,
            error_packet_count: optional_u64(object, known_keys[20])?,
        });
    }
    output.sort_by(|left, right| left.uuid.cmp(&right.uuid));
    if output
        .windows(2)
        .any(|window| window[0].uuid == window[1].uuid)
    {
        return Err(LiveError::SchemaViolation);
    }
    fields.sort();
    fields.dedup();
    Ok((output, fields))
}

fn present_fields(object: &Map<String, Value>, known_keys: &[&str]) -> Vec<String> {
    known_keys
        .iter()
        .filter(|key| object.contains_key(**key))
        .map(|key| (*key).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;

    struct FixtureClock {
        elapsed: Rc<Cell<Duration>>,
        sleeps: RefCell<Vec<Duration>>,
    }

    impl FixtureClock {
        fn new() -> Self {
            Self::with_elapsed(Rc::new(Cell::new(Duration::ZERO)))
        }

        fn with_elapsed(elapsed: Rc<Cell<Duration>>) -> Self {
            Self {
                elapsed,
                sleeps: RefCell::new(Vec::new()),
            }
        }
    }

    impl PollClock for FixtureClock {
        fn elapsed(&self) -> Duration {
            self.elapsed.get()
        }

        fn sleep(
            &mut self,
            duration: Duration,
            cancellation: &CancellationToken,
        ) -> Result<(), LiveError> {
            check_cancelled(cancellation)?;
            self.elapsed
                .set(self.elapsed.get().saturating_add(duration));
            self.sleeps.borrow_mut().push(duration);
            Ok(())
        }
    }

    struct FakeHttp {
        responses: RefCell<VecDeque<Result<RawResponse, LiveError>>>,
        requests: RefCell<Vec<(String, String, Duration)>>,
    }

    impl HttpGet for FakeHttp {
        fn get(
            &self,
            _endpoint: &Endpoint,
            _token: &ApiToken,
            path: &str,
            cookie: &str,
            _limits: &LiveLimits,
            remaining: Duration,
        ) -> Result<RawResponse, LiveError> {
            self.requests
                .borrow_mut()
                .push((path.to_owned(), cookie.to_owned(), remaining));
            self.responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(LiveError::Transport))
        }
    }

    enum FinalGetAction {
        Cancel(CancellationToken),
        Expire(Rc<Cell<Duration>>),
    }

    struct FinalGetHttp {
        responses: RefCell<VecDeque<Result<RawResponse, LiveError>>>,
        action: FinalGetAction,
        requests: RefCell<usize>,
    }

    impl HttpGet for FinalGetHttp {
        fn get(
            &self,
            _endpoint: &Endpoint,
            _token: &ApiToken,
            _path: &str,
            _cookie: &str,
            _limits: &LiveLimits,
            _remaining: Duration,
        ) -> Result<RawResponse, LiveError> {
            let mut requests = self.requests.borrow_mut();
            let request_number = *requests;
            *requests += 1;
            drop(requests);
            if request_number == 2 {
                match &self.action {
                    FinalGetAction::Cancel(cancellation) => cancellation.cancel(),
                    FinalGetAction::Expire(elapsed) => elapsed.set(Duration::from_secs(1)),
                }
            }
            self.responses
                .borrow_mut()
                .pop_front()
                .ok_or(LiveError::Transport)?
        }
    }

    fn successful_poll_responses() -> VecDeque<Result<RawResponse, LiveError>> {
        VecDeque::from([
            Ok(RawResponse {
                status: 200,
                body: br#"{"kismet.system.version":"2026.09.0-fixture","kismet.system.git":"2d25ad0"}"#.to_vec(),
            }),
            Ok(RawResponse {
                status: 200,
                body: br#"[{"kismet.datasource.driver.type":"linuxwifi"}]"#.to_vec(),
            }),
            Ok(RawResponse {
                status: 200,
                body: br#"[{"kismet.datasource.uuid":"source-a"}]"#.to_vec(),
            }),
        ])
    }

    fn limits() -> LiveLimits {
        LiveLimits {
            request_timeout: Duration::from_secs(1),
            retry_backoff: Duration::from_millis(20),
            ..LiveLimits::default()
        }
    }

    #[test]
    fn duplicate_keys_are_rejected_before_field_lookup() {
        assert_eq!(
            parse_json(br#"{"a":1,"a":2}"#, 8),
            Err(LiveError::DuplicateJsonKey)
        );
    }

    #[test]
    fn producer_version_requires_three_numeric_components() {
        assert_eq!(
            parse_producer_version("2026.09.0-build", &limits())
                .unwrap()
                .components(),
            (2026, 9, 0)
        );
        assert_eq!(
            parse_producer_version("future", &limits()),
            Err(LiveError::UnsupportedVersion)
        );
        assert_eq!(
            parse_producer_version("2026.09.1", &limits()),
            Err(LiveError::UnsupportedVersion)
        );
        assert_eq!(
            parse_producer_version("2030.01.0", &limits()),
            Err(LiveError::UnsupportedVersion)
        );
        assert_eq!(
            parse_producer_version("2026.1", &limits()),
            Err(LiveError::UnsupportedVersion)
        );
    }

    #[test]
    fn fields_are_sorted_and_unknown_optional_values_stay_absent() {
        let status = serde_json::json!({
            "kismet.system.version": "2026.09.0-build",
            "kismet.system.git": SUPPORTED_KISMET_GIT_SHORT,
            "kismet.system.devices.count": 3,
        });
        let (decoded, fields) = decode_status(&status, &limits()).unwrap();
        assert_eq!(decoded.device_count, Some(3));
        assert_eq!(decoded.clock_seconds, None);
        assert_eq!(
            fields,
            vec![
                "kismet.system.devices.count",
                "kismet.system.git",
                "kismet.system.version"
            ]
        );
        assert_eq!(
            decode_status(
                &serde_json::json!({"kismet.system.version":"2026.09.0-build"}),
                &limits()
            ),
            Err(LiveError::UnsupportedVersion)
        );
        let invalid_usecs = serde_json::json!({
            "kismet.system.version": "2026.09.0-build",
            "kismet.system.git": SUPPORTED_KISMET_GIT_SHORT,
            "kismet.system.timestamp.usec": 1_000_000,
        });
        assert_eq!(
            decode_status(&invalid_usecs, &limits()),
            Err(LiveError::SchemaViolation)
        );
    }

    #[test]
    fn source_order_and_duplicate_identity_are_deterministic() {
        let source = |uuid: &str| {
            serde_json::json!({
                "kismet.datasource.uuid": uuid,
                "kismet.datasource.remote": true,
                "kismet.datasource.channels": ["1", "6"],
            })
        };
        let (sources, _) =
            decode_sources(&Value::Array(vec![source("b"), source("a")]), &limits()).unwrap();
        assert_eq!(sources[0].uuid(), "a");
        assert_eq!(
            decode_sources(&Value::Array(vec![source("a"), source("a")]), &limits()),
            Err(LiveError::SchemaViolation)
        );
    }

    #[test]
    fn inventory_and_nesting_limits_are_enforced() {
        let mut constrained = limits();
        constrained.max_datasources = 1;
        let source = serde_json::json!({"kismet.datasource.uuid":"a"});
        assert_eq!(
            decode_sources(&Value::Array(vec![source.clone(), source]), &constrained),
            Err(LiveError::InventoryLimit)
        );
        assert_eq!(
            parse_json(br#"{"a":{"b":{"c":1}}}"#, 1),
            Err(LiveError::UnsupportedSchema)
        );
    }

    #[test]
    fn cancellation_token_is_stable_and_retry_delay_is_bounded() {
        let token = CancellationToken::default();
        let mut clock = FixtureClock::new();
        clock.sleep(Duration::from_millis(20), &token).unwrap();
        assert_eq!(
            clock.sleeps.borrow().as_slice(),
            &[Duration::from_millis(20)]
        );
        token.cancel();
        assert_eq!(
            clock.sleep(Duration::from_millis(20), &token),
            Err(LiveError::Cancelled)
        );
        assert_eq!(
            retry_delay(Duration::from_millis(20), 2),
            Duration::from_millis(80)
        );
    }

    #[test]
    fn transient_reconnects_use_injected_time_and_exact_read_only_paths() {
        let client = KismetLiveClient::connect(
            Endpoint::new("http://127.0.0.1:2501").unwrap(),
            ApiToken::new("fixture-secret").unwrap(),
            limits(),
        )
        .unwrap();
        let http = FakeHttp {
            responses: RefCell::new(VecDeque::from([
                Ok(RawResponse {
                    status: 503,
                    body: Vec::new(),
                }),
            Ok(RawResponse {
                status: 200,
                body: br#"{"kismet.system.version":"2026.09.0-fixture","kismet.system.git":"2d25ad0"}"#.to_vec(),
                }),
                Ok(RawResponse {
                    status: 200,
                    body: br#"[{"kismet.datasource.driver.type":"linuxwifi","kismet.datasource.driver.remote_capable":true}]"#.to_vec(),
                }),
                Ok(RawResponse {
                    status: 200,
                    body: br#"[{"kismet.datasource.uuid":"source-b","kismet.datasource.remote":true},{"kismet.datasource.uuid":"source-a","kismet.datasource.channel":null}]"#.to_vec(),
                }),
            ])),
            requests: RefCell::new(Vec::new()),
        };
        let cancellation = CancellationToken::default();
        let mut clock = FixtureClock::new();
        let snapshot = client
            .poll_with_executor(&http, &cancellation, &mut clock)
            .unwrap();
        assert_eq!(snapshot.datasources()[0].uuid(), "source-a");
        assert_eq!(
            http.requests
                .borrow()
                .iter()
                .map(|(path, _, _)| path.as_str())
                .collect::<Vec<_>>(),
            vec![STATUS_PATH, STATUS_PATH, TYPES_PATH, SOURCES_PATH]
        );
        assert!(
            http.requests
                .borrow()
                .iter()
                .all(|(_, cookie, _)| cookie == "KISMET=fixture-secret")
        );
        assert_eq!(
            http.requests.borrow()[0].2,
            Duration::from_nanos(333_333_333)
        );
        assert_eq!(
            http.requests.borrow()[1].2,
            Duration::from_nanos(333_333_333)
        );
        assert_eq!(
            clock.sleeps.borrow().as_slice(),
            &[Duration::from_millis(20)]
        );
        let bytes = snapshot.canonical_bytes();
        assert!(!String::from_utf8_lossy(&bytes).contains("fixture-secret"));
    }

    #[test]
    fn authentication_and_redirects_are_not_empty_successes() {
        let client = KismetLiveClient::connect(
            Endpoint::new("http://127.0.0.1:2501").unwrap(),
            ApiToken::new("secret").unwrap(),
            limits(),
        )
        .unwrap();
        for (status, error) in [
            (401, LiveError::AuthenticationRequired),
            (403, LiveError::Forbidden),
            (302, LiveError::Redirect),
        ] {
            let http = FakeHttp {
                responses: RefCell::new(VecDeque::from([Ok(RawResponse {
                    status,
                    body: b"[]".to_vec(),
                })])),
                requests: RefCell::new(Vec::new()),
            };
            let mut clock = FixtureClock::new();
            assert_eq!(
                client.poll_with_executor(&http, &CancellationToken::default(), &mut clock),
                Err(error)
            );
        }
    }

    #[test]
    fn transport_retries_are_bounded_and_cancellable() {
        let client = KismetLiveClient::connect(
            Endpoint::new("http://127.0.0.1:2501").unwrap(),
            ApiToken::new("secret").unwrap(),
            limits(),
        )
        .unwrap();
        let http = FakeHttp {
            responses: RefCell::new(VecDeque::from([
                Err(LiveError::Transport),
                Err(LiveError::Transport),
                Err(LiveError::Transport),
            ])),
            requests: RefCell::new(Vec::new()),
        };
        let mut clock = FixtureClock::new();
        assert_eq!(
            client.poll_with_executor(&http, &CancellationToken::default(), &mut clock),
            Err(LiveError::RetryExhausted)
        );
        assert_eq!(http.requests.borrow().len(), 3);
        assert_eq!(
            clock.sleeps.borrow().as_slice(),
            &[Duration::from_millis(20), Duration::from_millis(40)]
        );
        let cancelled = CancellationToken::default();
        cancelled.cancel();
        assert_eq!(
            client.poll_with_executor(&http, &cancelled, &mut FixtureClock::new()),
            Err(LiveError::Cancelled)
        );
    }

    #[test]
    fn final_response_after_cancellation_is_not_published() {
        let client = KismetLiveClient::connect(
            Endpoint::new("http://127.0.0.1:2501").unwrap(),
            ApiToken::new("secret").unwrap(),
            limits(),
        )
        .unwrap();
        let cancellation = CancellationToken::default();
        let http = FinalGetHttp {
            responses: RefCell::new(successful_poll_responses()),
            action: FinalGetAction::Cancel(cancellation.clone()),
            requests: RefCell::new(0),
        };
        assert_eq!(
            client.poll_with_executor(&http, &cancellation, &mut FixtureClock::new()),
            Err(LiveError::Cancelled)
        );
    }

    #[test]
    fn final_response_at_deadline_is_not_published() {
        let client = KismetLiveClient::connect(
            Endpoint::new("http://127.0.0.1:2501").unwrap(),
            ApiToken::new("secret").unwrap(),
            limits(),
        )
        .unwrap();
        let elapsed = Rc::new(Cell::new(Duration::ZERO));
        let http = FinalGetHttp {
            responses: RefCell::new(successful_poll_responses()),
            action: FinalGetAction::Expire(elapsed.clone()),
            requests: RefCell::new(0),
        };
        let mut clock = FixtureClock::with_elapsed(elapsed);
        assert_eq!(
            client.poll_with_executor(&http, &CancellationToken::default(), &mut clock),
            Err(LiveError::DeadlineExceeded)
        );
    }
}
