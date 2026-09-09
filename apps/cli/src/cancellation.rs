use kyberia_project_store::Cancellation;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// A cancellation source safe to poll from storage and numerical work.
/// Signal handlers only set this atomic flag; all application work remains
/// outside the async-signal-safe handler context.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    #[cfg(test)]
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

impl Cancellation for CancellationToken {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// The CLI's SIGINT registration is intentionally isolated from the
/// cancellation token. This keeps the token useful for deterministic tests
/// and future job adapters without exposing signal-library types inward.
pub struct ProcessCancellation {
    token: CancellationToken,
    #[cfg(unix)]
    _signal_id: signal_hook::SigId,
}

impl ProcessCancellation {
    pub const fn capability() -> &'static str {
        if cfg!(unix) { "sigint" } else { "unavailable" }
    }

    #[cfg(unix)]
    pub fn install() -> Result<Self, Box<dyn std::error::Error>> {
        let token = CancellationToken::default();
        let signal_id =
            signal_hook::flag::register(signal_hook::consts::SIGINT, token.cancelled.clone())?;
        Ok(Self {
            token,
            _signal_id: signal_id,
        })
    }

    #[cfg(not(unix))]
    pub fn install() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            token: CancellationToken::default(),
        })
    }

    pub fn token(&self) -> &CancellationToken {
        &self.token
    }
}

#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("stored RSSI analysis cancelled before publication")
    }
}

impl std::error::Error for Cancelled {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_transitions_from_live_to_cancelled() {
        let token = CancellationToken::default();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }
}
