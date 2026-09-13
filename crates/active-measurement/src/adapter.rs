//! Production standard-library adapters for active execution.

use crate::executor::{ConnectResult, MonotonicClock, TcpConnector};
use kyberia_domain::active::{ActiveIpAddress, ActiveSocketAddr};
use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream},
    time::{Duration, Instant},
};

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

    fn sleep_for(&mut self, duration: Duration) {
        std::thread::sleep(duration);
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
    fn connect(&mut self, target: ActiveSocketAddr, timeout: Duration) -> ConnectResult {
        match TcpStream::connect_timeout(&to_std_address(target), timeout) {
            Ok(stream) => {
                drop(stream);
                ConnectResult::Connected
            }
            Err(error) => classify(error),
        }
    }
}
