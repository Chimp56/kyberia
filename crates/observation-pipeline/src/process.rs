//! Bounded supervision of an explicitly trusted native collector process.
//!
//! This is an outward composition boundary. The process protocol is decoded
//! and normalized by `kyberia-capture-adapter`; only the resulting canonical
//! batch crosses into the point-survey and project-store pipeline. The
//! supervisor never requests permissions, manufactures IDs/clocks/poses, or
//! accepts arbitrary argv fragments.

use crate::{
    BatchError, Cancellation, PipelineError, PipelineOutcome, PipelineRequest,
    ReceivedObservationBatch, ingest,
};
use kyberia_capture_adapter::macos::{
    DecodedStream, MappingContext, NormalizedCapture, TerminalStatus, decode, normalize,
};
use kyberia_domain::identity::ContentHash;
use kyberia_project_store::Bundle;
use kyberia_survey::PointSurvey;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    io::Errno,
    process::{Pid, Signal, kill_process_group},
};
#[cfg(unix)]
use std::os::fd::AsFd;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::{
    io::{self, Read},
    process::{Child, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

/// The collector's bounded stderr diagnostic stream. It is never persisted as
/// canonical evidence and is intentionally not returned to callers.
pub const MAX_STDERR_BYTES: usize = 64 * 1024;
#[cfg(unix)]
const PIPE_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustError {
    RelativePath,
    MissingOrInaccessible,
    Symlink,
    NotRegularFile,
    NotExecutable,
}

impl std::fmt::Display for TrustError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::RelativePath => "trusted collector path must be absolute",
            Self::MissingOrInaccessible => "trusted collector is inaccessible",
            Self::Symlink => "trusted collector path may not be a symlink",
            Self::NotRegularFile => "trusted collector path is not a regular file",
            Self::NotExecutable => "trusted collector is not executable",
        })
    }
}

impl std::error::Error for TrustError {}

/// An operator-selected collector executable and the source-build hash that
/// its hello record must declare. Construction is explicit so callers cannot
/// accidentally turn an untrusted string into a plugin capability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedCollector {
    path: PathBuf,
    expected_source_build: ContentHash,
}

impl TrustedCollector {
    pub fn new(
        path: impl AsRef<Path>,
        expected_source_build: ContentHash,
    ) -> Result<Self, TrustError> {
        let path = path.as_ref().to_owned();
        validate_trusted_path(&path)?;
        Ok(Self {
            path,
            expected_source_build,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn expected_source_build(&self) -> ContentHash {
        self.expected_source_build
    }
}

fn validate_trusted_path(path: &Path) -> Result<(), TrustError> {
    if !path.is_absolute() {
        return Err(TrustError::RelativePath);
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| TrustError::MissingOrInaccessible)?;
    if metadata.file_type().is_symlink() {
        return Err(TrustError::Symlink);
    }
    if !metadata.is_file() {
        return Err(TrustError::NotRegularFile);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(TrustError::NotExecutable);
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectorOptionError {
    TimeoutOutOfRange,
    LimitOutOfRange,
    InvalidInterface,
}

impl std::fmt::Display for CollectorOptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TimeoutOutOfRange => "collector timeout must be between 1 and 60 seconds",
            Self::LimitOutOfRange => "collector observation limit must be between 1 and 4096",
            Self::InvalidInterface => "collector interface must be a bounded ASCII name",
        })
    }
}

impl std::error::Error for CollectorOptionError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeOptions {
    timeout_seconds: u8,
}

impl ProbeOptions {
    pub fn new(timeout_seconds: u8) -> Result<Self, CollectorOptionError> {
        if !(1..=60).contains(&timeout_seconds) {
            return Err(CollectorOptionError::TimeoutOutOfRange);
        }
        Ok(Self { timeout_seconds })
    }

    pub const fn timeout_seconds(self) -> u8 {
        self.timeout_seconds
    }
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            timeout_seconds: 20,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanOptions {
    interface: Option<String>,
    limit: u16,
    timeout_seconds: u8,
    include_identifiers: bool,
}

impl ScanOptions {
    pub fn new(
        interface: Option<String>,
        limit: u16,
        timeout_seconds: u8,
        include_identifiers: bool,
    ) -> Result<Self, CollectorOptionError> {
        if !(1..=4096).contains(&limit) {
            return Err(CollectorOptionError::LimitOutOfRange);
        }
        if !(1..=60).contains(&timeout_seconds) {
            return Err(CollectorOptionError::TimeoutOutOfRange);
        }
        if interface.as_deref().is_some_and(|value| {
            value.is_empty()
                || value.len() > 64
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        }) {
            return Err(CollectorOptionError::InvalidInterface);
        }
        Ok(Self {
            interface,
            limit,
            timeout_seconds,
            include_identifiers,
        })
    }

    pub fn interface(&self) -> Option<&str> {
        self.interface.as_deref()
    }

    pub const fn limit(&self) -> u16 {
        self.limit
    }

    pub const fn timeout_seconds(&self) -> u8 {
        self.timeout_seconds
    }

    pub const fn includes_identifiers(&self) -> bool {
        self.include_identifiers
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CollectorCommand {
    Probe(ProbeOptions),
    Scan(ScanOptions),
}

impl CollectorCommand {
    fn timeout_seconds(&self) -> u8 {
        match self {
            Self::Probe(options) => options.timeout_seconds(),
            Self::Scan(options) => options.timeout_seconds(),
        }
    }

    fn argv(&self) -> Vec<OsString> {
        let mut args = Vec::new();
        match self {
            Self::Probe(options) => {
                args.push(OsString::from("probe"));
                args.push(OsString::from("--timeout-seconds"));
                args.push(OsString::from(options.timeout_seconds().to_string()));
            }
            Self::Scan(options) => {
                args.push(OsString::from("scan"));
                if let Some(interface) = options.interface() {
                    args.push(OsString::from("--interface"));
                    args.push(OsString::from(interface));
                }
                args.push(OsString::from("--limit"));
                args.push(OsString::from(options.limit().to_string()));
                args.push(OsString::from("--timeout-seconds"));
                args.push(OsString::from(options.timeout_seconds().to_string()));
                if options.includes_identifiers() {
                    args.push(OsString::from("--include-identifiers"));
                }
            }
        }
        args
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NativeCaptureSessionError {
    Trust(TrustError),
    Option(CollectorOptionError),
    Cancelled,
    Timeout,
    OutputLimit(OutputStream),
    ProcessIo,
    ProcessCleanup,
    UnsupportedPlatform,
    ProcessExitWithoutCode,
    TerminalExitMismatch {
        terminal: TerminalStatus,
        exit_code: i32,
    },
    CommandProvenanceMismatch,
    IdentifierPolicyMismatch,
    ObservationLimitMismatch,
    InterfaceProvenanceMismatch,
    SourceBuildMismatch,
    MappingRejected,
    AdapterDecode(kyberia_capture_adapter::Error),
    AdapterNormalize(kyberia_capture_adapter::Error),
    Batch(BatchError),
    Pipeline(PipelineError),
}

impl std::fmt::Display for NativeCaptureSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Trust(error) => error.fmt(f),
            Self::Option(error) => error.fmt(f),
            Self::Cancelled => f.write_str("native collector session cancelled"),
            Self::Timeout => f.write_str("native collector session timed out"),
            Self::OutputLimit(stream) => {
                write!(f, "native collector {stream:?} output exceeded its bound")
            }
            Self::ProcessIo => f.write_str("native collector process I/O failed"),
            Self::ProcessCleanup => f.write_str("native collector process cleanup failed"),
            Self::UnsupportedPlatform => {
                f.write_str("native collector process supervision is unsupported on this platform")
            }
            Self::ProcessExitWithoutCode => {
                f.write_str("native collector exited without a status code")
            }
            Self::TerminalExitMismatch {
                terminal,
                exit_code,
            } => {
                write!(
                    f,
                    "native collector terminal {terminal:?} disagrees with exit {exit_code}"
                )
            }
            Self::CommandProvenanceMismatch => {
                f.write_str("native collector hello does not match the typed command")
            }
            Self::IdentifierPolicyMismatch => {
                f.write_str("native collector identifier policy does not match the typed command")
            }
            Self::ObservationLimitMismatch => {
                f.write_str("native collector returned more observations than requested")
            }
            Self::InterfaceProvenanceMismatch => {
                f.write_str("native collector observations came from a different interface")
            }
            Self::SourceBuildMismatch => {
                f.write_str("native collector source build is not trusted")
            }
            Self::MappingRejected => f.write_str("native collector identity mapping was rejected"),
            Self::AdapterDecode(error) => error.fmt(f),
            Self::AdapterNormalize(error) => error.fmt(f),
            Self::Batch(error) => error.fmt(f),
            Self::Pipeline(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for NativeCaptureSessionError {}

impl From<TrustError> for NativeCaptureSessionError {
    fn from(value: TrustError) -> Self {
        Self::Trust(value)
    }
}

impl From<CollectorOptionError> for NativeCaptureSessionError {
    fn from(value: CollectorOptionError) -> Self {
        Self::Option(value)
    }
}

/// The complete process and durable-pipeline result. The normalized capture
/// is retained for callers that need capability/terminal provenance; its
/// canonical observations are the same values persisted by `pipeline`.
#[derive(Clone, Debug)]
pub struct NativeCaptureSessionOutcome {
    pub process_session: String,
    pub terminal: TerminalStatus,
    pub exit_code: i32,
    pub normalized: NormalizedCapture,
    pub pipeline: PipelineOutcome,
}

/// Launch a trusted macOS collector, normalize its bounded NDJSON stream and
/// persist the resulting point snapshot. The mapping callback must provide
/// explicit canonical identities for the process session, source and
/// observations; this function never derives them from foreign fields.
pub fn run_and_persist<C, F>(
    bundle: &mut Bundle,
    collector: &TrustedCollector,
    command: CollectorCommand,
    mapping: F,
    survey: &PointSurvey,
    request: &PipelineRequest,
    cancel: &C,
) -> Result<NativeCaptureSessionOutcome, NativeCaptureSessionError>
where
    C: Cancellation,
    F: FnOnce(&DecodedStream) -> Result<MappingContext, NativeCaptureSessionError>,
{
    if cancel.is_cancelled() {
        return Err(NativeCaptureSessionError::Cancelled);
    }
    let (stdout, exit_code) = run_process(collector, &command, cancel)?;
    if cancel.is_cancelled() {
        return Err(NativeCaptureSessionError::Cancelled);
    }
    let stream = decode(&stdout).map_err(NativeCaptureSessionError::AdapterDecode)?;
    let (expected_command, expected_identifiers, expected_limit, expected_interface) =
        match &command {
            CollectorCommand::Probe(_) => ("probe", false, None, None),
            CollectorCommand::Scan(options) => (
                "scan",
                options.includes_identifiers(),
                Some(options.limit()),
                options.interface(),
            ),
        };
    if stream.command_name() != expected_command
        || stream.declared_timeout_seconds() != command.timeout_seconds()
    {
        return Err(NativeCaptureSessionError::CommandProvenanceMismatch);
    }
    if stream.collector_build() != collector.expected_source_build() {
        return Err(NativeCaptureSessionError::SourceBuildMismatch);
    }
    if stream.identifiers_included() != expected_identifiers {
        return Err(NativeCaptureSessionError::IdentifierPolicyMismatch);
    }
    if let Some(limit) = expected_limit {
        if stream.observation_keys().count() > usize::from(limit) {
            return Err(NativeCaptureSessionError::ObservationLimitMismatch);
        }
        // Capabilities may list several radios. Bind an explicitly requested
        // interface only to the source attached to observations; empty and
        // error captures carry no active-source evidence to compare.
        if let Some(requested) = expected_interface {
            let observed = stream.observation_sources().collect::<Vec<_>>();
            if !observed.is_empty()
                && !observed
                    .iter()
                    .any(|(_, interface)| *interface == requested)
            {
                return Err(NativeCaptureSessionError::InterfaceProvenanceMismatch);
            }
        }
    }
    let context = mapping(&stream)?;
    let normalized =
        normalize(&stream, &context).map_err(NativeCaptureSessionError::AdapterNormalize)?;
    let terminal = normalized.completion.status;
    if !terminal_matches_exit(terminal, exit_code) {
        return Err(NativeCaptureSessionError::TerminalExitMismatch {
            terminal,
            exit_code,
        });
    }
    if cancel.is_cancelled() {
        return Err(NativeCaptureSessionError::Cancelled);
    }
    let batch = ReceivedObservationBatch::from_normalized_capture(normalized.clone())
        .map_err(NativeCaptureSessionError::Batch)?;
    let pipeline = ingest(bundle, survey, &batch, request, cancel)
        .map_err(NativeCaptureSessionError::Pipeline)?;
    Ok(NativeCaptureSessionOutcome {
        process_session: stream.process_session().to_owned(),
        terminal,
        exit_code,
        normalized,
        pipeline,
    })
}

fn terminal_matches_exit(terminal: TerminalStatus, exit_code: i32) -> bool {
    match terminal {
        TerminalStatus::Ok => exit_code == 0,
        TerminalStatus::Partial => exit_code == 2,
        TerminalStatus::PermissionRequired => exit_code == 77,
        TerminalStatus::Unsupported | TerminalStatus::Unavailable => exit_code == 69,
        TerminalStatus::Error => exit_code == 70,
        TerminalStatus::Timeout => exit_code == 124,
        TerminalStatus::Cancelled => exit_code == 130 || exit_code == 143,
    }
}

#[cfg(unix)]
fn run_process<C: Cancellation>(
    collector: &TrustedCollector,
    command: &CollectorCommand,
    cancel: &C,
) -> Result<(Vec<u8>, i32), NativeCaptureSessionError> {
    validate_trusted_path(collector.path())?;
    let mut command_builder = Command::new(collector.path());
    command_builder
        .args(command.argv())
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command_builder.process_group(0);
    let mut child = command_builder
        .spawn()
        .map_err(|_| NativeCaptureSessionError::ProcessIo)?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            return Err(terminate_or(
                &mut child,
                NativeCaptureSessionError::ProcessIo,
            ));
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            return Err(terminate_or(
                &mut child,
                NativeCaptureSessionError::ProcessIo,
            ));
        }
    };
    let stdout_overflow = Arc::new(AtomicBool::new(false));
    let stderr_overflow = Arc::new(AtomicBool::new(false));
    let reader_stop = Arc::new(AtomicBool::new(false));
    let stdout_reader = bounded_reader(
        stdout,
        kyberia_capture_adapter::macos::MAX_STREAM_BYTES,
        stdout_overflow.clone(),
        reader_stop.clone(),
    );
    let stderr_reader = bounded_reader(
        stderr,
        MAX_STDERR_BYTES,
        stderr_overflow.clone(),
        reader_stop.clone(),
    );
    let deadline = Instant::now() + Duration::from_secs(u64::from(command.timeout_seconds()));
    let mut termination = None;
    let child_id = child.id();
    let status = loop {
        if stdout_overflow.load(Ordering::Acquire) {
            reader_stop.store(true, Ordering::Release);
            termination = Some(terminate_or(
                &mut child,
                NativeCaptureSessionError::OutputLimit(OutputStream::Stdout),
            ));
            break None;
        }
        if stderr_overflow.load(Ordering::Acquire) {
            reader_stop.store(true, Ordering::Release);
            termination = Some(terminate_or(
                &mut child,
                NativeCaptureSessionError::OutputLimit(OutputStream::Stderr),
            ));
            break None;
        }
        if cancel.is_cancelled() {
            reader_stop.store(true, Ordering::Release);
            termination = Some(terminate_or(
                &mut child,
                NativeCaptureSessionError::Cancelled,
            ));
            break None;
        }
        if Instant::now() >= deadline {
            reader_stop.store(true, Ordering::Release);
            termination = Some(terminate_or(&mut child, NativeCaptureSessionError::Timeout));
            break None;
        }
        match child.try_wait() {
            Err(_) => {
                reader_stop.store(true, Ordering::Release);
                termination = Some(terminate_or(
                    &mut child,
                    NativeCaptureSessionError::ProcessIo,
                ));
                break None;
            }
            Ok(Some(status)) => break Some(status),
            Ok(None) => thread::sleep(Duration::from_millis(2)),
        }
    };
    let mut stdout_result = receive_reader(&stdout_reader, PIPE_DRAIN_TIMEOUT);
    let mut stderr_result = receive_reader(&stderr_reader, PIPE_DRAIN_TIMEOUT);
    let mut drain_cleanup_failed = false;
    let mut drain_forced = false;
    if stdout_result.is_none() || stderr_result.is_none() {
        // A direct child can exit while a descendant still owns one of its
        // pipes. The process group is ours, so stop the readers and terminate
        // that group before a second bounded drain.
        reader_stop.store(true, Ordering::Release);
        drain_forced = true;
        drain_cleanup_failed = terminate_process_group(child_id).is_err();
        if stdout_result.is_none() {
            stdout_result = receive_reader(&stdout_reader, PIPE_DRAIN_TIMEOUT);
        }
        if stderr_result.is_none() {
            stderr_result = receive_reader(&stderr_reader, PIPE_DRAIN_TIMEOUT);
        }
    }
    if stdout_result.is_none() {
        stdout_result = receive_reader(&stdout_reader, PIPE_DRAIN_TIMEOUT);
    }
    if stderr_result.is_none() {
        stderr_result = receive_reader(&stderr_reader, PIPE_DRAIN_TIMEOUT);
    }
    let stdout_tail = if stdout_result.is_none() {
        finish_reader(&stdout_reader)
    } else {
        None
    };
    let stderr_tail = if stderr_result.is_none() {
        finish_reader(&stderr_reader)
    } else {
        None
    };
    let stdout_capture = stdout_result.or(stdout_tail);
    let stderr_capture = stderr_result.or(stderr_tail);
    let stdout_received = stdout_capture.is_some();
    let stderr_received = stderr_capture.is_some();
    let stdout_join = join_reader(stdout_reader, stdout_received);
    let stderr_join = join_reader(stderr_reader, stderr_received);
    stdout_join?;
    stderr_join?;
    if let Some(error) = termination {
        return Err(error);
    }
    if drain_cleanup_failed {
        return Err(NativeCaptureSessionError::ProcessCleanup);
    }
    if drain_forced {
        return Err(NativeCaptureSessionError::ProcessIo);
    }
    if stdout_overflow.load(Ordering::Acquire) {
        return Err(NativeCaptureSessionError::OutputLimit(OutputStream::Stdout));
    }
    if stderr_overflow.load(Ordering::Acquire) {
        return Err(NativeCaptureSessionError::OutputLimit(OutputStream::Stderr));
    }
    let stdout = stdout_capture
        .ok_or(NativeCaptureSessionError::ProcessIo)?
        .map_err(|_| NativeCaptureSessionError::ProcessIo)?;
    let stderr = stderr_capture
        .ok_or(NativeCaptureSessionError::ProcessIo)?
        .map_err(|_| NativeCaptureSessionError::ProcessIo)?;
    if stdout.stopped || stderr.stopped {
        return Err(NativeCaptureSessionError::ProcessIo);
    }
    let status = status.ok_or(NativeCaptureSessionError::ProcessIo)?;
    let exit_code = status
        .code()
        .ok_or(NativeCaptureSessionError::ProcessExitWithoutCode)?;
    Ok((stdout.bytes, exit_code))
}

#[cfg(unix)]
fn terminate_or(child: &mut Child, error: NativeCaptureSessionError) -> NativeCaptureSessionError {
    match terminate_and_reap(child) {
        Ok(()) => error,
        Err(cleanup) => cleanup,
    }
}

#[cfg(unix)]
fn terminate_and_reap(child: &mut Child) -> Result<(), NativeCaptureSessionError> {
    let group_failed = terminate_process_group(child.id()).is_err();
    let child_kill_failed = child
        .kill()
        .map_or_else(|error| error.kind() != io::ErrorKind::NotFound, |_| false);
    child
        .wait()
        .map(|_| ())
        .map_err(|_| NativeCaptureSessionError::ProcessCleanup)?;
    if group_failed || child_kill_failed {
        Err(NativeCaptureSessionError::ProcessCleanup)
    } else {
        Ok(())
    }
}

#[cfg(unix)]
struct Reader {
    receiver: Receiver<io::Result<ReadCapture>>,
    handle: Option<thread::JoinHandle<()>>,
}

#[cfg(unix)]
struct ReadCapture {
    bytes: Vec<u8>,
    stopped: bool,
}

#[cfg(unix)]
fn bounded_reader<R>(
    mut reader: R,
    limit: usize,
    overflow: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> Reader
where
    R: Read + Send + 'static + AsFd,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        let result = bounded_read_poll(&mut reader, limit, overflow, stop);
        let _ = sender.send(result);
    });
    Reader {
        receiver,
        handle: Some(handle),
    }
}

/// The native process contract is POSIX-only. Keeping the unsupported target
/// explicit prevents a platform with blocking pipe reads from accidentally
/// inheriting the macOS lifecycle guarantees.
#[cfg(not(unix))]
fn run_process<C: Cancellation>(
    _collector: &TrustedCollector,
    _command: &CollectorCommand,
    _cancel: &C,
) -> Result<(Vec<u8>, i32), NativeCaptureSessionError> {
    Err(NativeCaptureSessionError::UnsupportedPlatform)
}

#[cfg(unix)]
fn bounded_read_poll<R: Read + AsFd>(
    reader: &mut R,
    limit: usize,
    overflow: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> io::Result<ReadCapture> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0_u8; 8192];
    let timeout = Timespec {
        tv_sec: 0,
        tv_nsec: 20_000_000,
    };
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(ReadCapture {
                bytes,
                stopped: true,
            });
        }
        let mut descriptor = [PollFd::new(reader, PollFlags::IN | PollFlags::HUP)];
        if poll(&mut descriptor, Some(&timeout)).is_err() {
            return Err(io::Error::other("collector pipe poll failed"));
        }
        if descriptor[0].revents().is_empty() {
            continue;
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(ReadCapture {
                bytes,
                stopped: false,
            });
        }
        let remaining = limit.saturating_sub(bytes.len());
        let kept = read.min(remaining);
        bytes.extend_from_slice(&buffer[..kept]);
        if kept != read {
            overflow.store(true, Ordering::Release);
        }
    }
}

#[cfg(unix)]
fn receive_reader(reader: &Reader, timeout: Duration) -> Option<io::Result<ReadCapture>> {
    reader.receiver.recv_timeout(timeout).ok()
}

#[cfg(unix)]
fn finish_reader(reader: &Reader) -> Option<io::Result<ReadCapture>> {
    reader.receiver.recv_timeout(PIPE_DRAIN_TIMEOUT).ok()
}

#[cfg(unix)]
fn join_reader(mut reader: Reader, result_received: bool) -> Result<(), NativeCaptureSessionError> {
    let Some(handle) = reader.handle.take() else {
        return Ok(());
    };
    if !result_received && !handle.is_finished() {
        return Err(NativeCaptureSessionError::ProcessIo);
    }
    handle
        .join()
        .map_err(|_| NativeCaptureSessionError::ProcessCleanup)
}

#[cfg(unix)]
fn terminate_process_group(process_id: u32) -> Result<(), Errno> {
    let Some(pid) = Pid::from_raw(process_id as i32) else {
        return Ok(());
    };
    match kill_process_group(pid, Signal::KILL) {
        Ok(()) | Err(Errno::SRCH) => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_options_reject_unbounded_or_stringly_arguments() {
        assert_eq!(
            ProbeOptions::new(0),
            Err(CollectorOptionError::TimeoutOutOfRange)
        );
        assert_eq!(
            ScanOptions::new(None, 0, 1, false),
            Err(CollectorOptionError::LimitOutOfRange)
        );
        assert_eq!(
            ScanOptions::new(Some("en0;rm -rf".to_owned()), 1, 1, false),
            Err(CollectorOptionError::InvalidInterface)
        );
        let scan = ScanOptions::new(Some("en0".to_owned()), 256, 20, true).unwrap();
        assert_eq!(
            CollectorCommand::Scan(scan).argv(),
            vec![
                OsString::from("scan"),
                OsString::from("--interface"),
                OsString::from("en0"),
                OsString::from("--limit"),
                OsString::from("256"),
                OsString::from("--timeout-seconds"),
                OsString::from("20"),
                OsString::from("--include-identifiers"),
            ]
        );
    }

    #[test]
    fn terminal_and_exit_status_are_a_closed_mapping() {
        assert!(terminal_matches_exit(TerminalStatus::Ok, 0));
        assert!(terminal_matches_exit(TerminalStatus::Partial, 2));
        assert!(terminal_matches_exit(
            TerminalStatus::PermissionRequired,
            77
        ));
        assert!(terminal_matches_exit(TerminalStatus::Unsupported, 69));
        assert!(terminal_matches_exit(TerminalStatus::Error, 70));
        assert!(terminal_matches_exit(TerminalStatus::Timeout, 124));
        assert!(terminal_matches_exit(TerminalStatus::Cancelled, 130));
        assert!(!terminal_matches_exit(TerminalStatus::Ok, 70));
        assert!(!terminal_matches_exit(TerminalStatus::Partial, 0));
    }

    #[test]
    fn trusted_path_rejects_relative_non_regular_and_non_executable_paths() {
        let hash = ContentHash::from_sha256([1; 32]);
        assert_eq!(
            TrustedCollector::new("relative", hash),
            Err(TrustError::RelativePath)
        );
        assert_eq!(
            TrustedCollector::new("/definitely/missing/collector", hash),
            Err(TrustError::MissingOrInaccessible)
        );
    }
}
