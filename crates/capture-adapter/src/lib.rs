//! Outward, deterministic normalization of native collector evidence.
//! No native calls, permissions, storage, or invented measurement timing.
mod json;
pub mod macos;
mod wire;

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    SizeLimit,
    RecordLimit,
    Truncated,
    InvalidJson,
    UnsupportedProtocol,
    InvalidField(&'static str),
    Sequence,
    Provenance,
    Capability,
    Incomplete,
    MissingMapping(&'static str),
    Privacy,
    Canonical,
}

/// Error messages contain no imported network identifiers or parser excerpts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub record: Option<usize>,
    pub kind: ErrorKind,
}
impl Error {
    fn new(kind: ErrorKind) -> Self {
        Self { record: None, kind }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "capture adapter {:?} at record {:?}",
            self.kind, self.record
        )
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;
fn check(ok: bool, kind: ErrorKind) -> Result<()> {
    if ok { Ok(()) } else { Err(Error::new(kind)) }
}
