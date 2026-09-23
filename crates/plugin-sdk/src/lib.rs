//! Portable declarations and validation for collector, metric, and export plugins.
//!
//! This crate validates manifests and negotiates versioned host contracts. It
//! does not load or execute components and does not enforce a sandbox, grants,
//! cancellation, or resource limits.

mod model;
mod registry;
mod validation;

pub use model::*;
pub use registry::*;
pub use validation::*;
