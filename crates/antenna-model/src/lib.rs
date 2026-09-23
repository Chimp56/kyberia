//! Versioned antenna-pattern data with validation before numerical evaluation.
//!
//! V1 accepts one open representation: frequency-indexed, full-sphere tabulated
//! gain in a fixed coordinate frame. It does not import manufacturer-specific
//! formats or ship a vendor catalog.

mod contract;
mod evaluate;

pub use contract::*;
pub use evaluate::*;

/// Structural JSON Schema for the first open antenna-pattern wire contract.
pub const JSON_SCHEMA_V1: &str = include_str!("../schema/antenna-pattern-v1.schema.json");
