//! Safe active-measurement boundary.
//!
//! The pure scheduling/statistics contracts are separate from the generic
//! executor and the production std TCP adapter.  No endpoint is resolved by
//! DNS, no process is spawned, and no arbitrary command text is accepted.
pub mod adapter;
pub mod executor;
pub mod pure;

pub use adapter::{StdMonotonicClock, StdTcpConnector};
pub use executor::{
    ActiveExecutionReport, ActiveMeasurementError, Cancellation, ConnectResult, MonotonicClock,
    NeverCancelled, TcpConnector, execute,
};
pub use pure::{ActiveSchedule, SCHEDULE_VERSION, ScheduleError, ScheduledSample, build_schedule};
