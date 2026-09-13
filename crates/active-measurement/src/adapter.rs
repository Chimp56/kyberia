//! Production OS-backed adapters for active execution.

use crate::executor::{Cancellation, ConnectResult, MonotonicClock, SleepResult, TcpConnector};
use kyberia_domain::active::{ActiveIpAddress, ActiveSocketAddr};
use mio::{Events, Interest, Poll, Token};
use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::{Duration, Instant},
};

const CANCELLATION_POLL: Duration = Duration::from_millis(25);

/// A monotonic clock backed by `std::time::Instant`. The epoch is supplied by
/// the application in the canonical provenance record; this adapter only
/// reports elapsed nanoseconds from its construction origin.
#[derive(Debug)]
pub struct StdMonotonicClock {
    origin: Instant,
}

impl Default for StdMonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl StdMonotonicClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl MonotonicClock for StdMonotonicClock {
    fn now_nanos(&mut self) -> u64 {
        self.origin.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
    }

    fn sleep_for(&mut self, duration: Duration, cancellation: &dyn Cancellation) -> SleepResult {
        if cancellation.is_cancelled() {
            return SleepResult::Cancelled;
        }
        let started = Instant::now();
        loop {
            let elapsed = started.elapsed();
            if elapsed >= duration {
                return SleepResult::Complete;
            }
            std::thread::sleep(CANCELLATION_POLL.min(duration - elapsed));
            if cancellation.is_cancelled() {
                return SleepResult::Cancelled;
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct StdTcpConnector;

impl StdTcpConnector {
    pub const fn new() -> Self {
        Self
    }
}

fn to_std_address(address: ActiveSocketAddr) -> SocketAddr {
    let ip = match address.address() {
        ActiveIpAddress::V4(octets) => IpAddr::V4(Ipv4Addr::from(octets)),
        ActiveIpAddress::V6(octets) => IpAddr::V6(Ipv6Addr::from(octets)),
    };
    SocketAddr::new(ip, address.port())
}

fn classify(error: io::Error) -> ConnectResult {
    match error.kind() {
        io::ErrorKind::ConnectionRefused => ConnectResult::ConnectionRefused,
        io::ErrorKind::TimedOut => ConnectResult::Timeout,
        io::ErrorKind::PermissionDenied => ConnectResult::PermissionDenied,
        io::ErrorKind::AddrNotAvailable
        | io::ErrorKind::NetworkUnreachable
        | io::ErrorKind::HostUnreachable => ConnectResult::Unreachable,
        _ => ConnectResult::Error,
    }
}

impl TcpConnector for StdTcpConnector {
    fn connect(
        &mut self,
        target: ActiveSocketAddr,
        timeout: Duration,
        cancellation: &dyn Cancellation,
    ) -> ConnectResult {
        if cancellation.is_cancelled() {
            return ConnectResult::Cancelled;
        }
        let address = to_std_address(target);
        let mut stream = match mio::net::TcpStream::connect(address) {
            Ok(stream) => stream,
            Err(error) => return classify(error),
        };
        let mut poll = match Poll::new() {
            Ok(poll) => poll,
            Err(error) => return classify(error),
        };
        if let Err(error) = poll.registry().register(
            &mut stream,
            Token(0),
            Interest::READABLE | Interest::WRITABLE,
        ) {
            return classify(error);
        }
        let mut events = Events::with_capacity(1);
        let started = Instant::now();
        loop {
            if cancellation.is_cancelled() {
                return ConnectResult::Cancelled;
            }
            let elapsed = started.elapsed();
            if elapsed >= timeout {
                return ConnectResult::Timeout;
            }
            let wait = CANCELLATION_POLL.min(timeout - elapsed);
            match poll.poll(&mut events, Some(wait)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return classify(error),
            }
            if events.iter().any(|event| event.token() == Token(0)) {
                match stream.take_error() {
                    Ok(Some(error)) => return classify(error),
                    Err(error)
                        if error.kind() == io::ErrorKind::NotConnected
                            || error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return classify(error),
                    Ok(None) => {}
                }
                match stream.peer_addr() {
                    Ok(_) => return ConnectResult::Connected,
                    Err(error)
                        if error.kind() == io::ErrorKind::NotConnected
                            || error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return classify(error),
                }
            }
        }
    }
}
