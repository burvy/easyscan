//! EasyScan image pipeline: page detection, filters and PDF export.
//!
//! Pure Rust with no platform code, so it is tested on desktop and exposed to
//! the Flutter app through a thin flutter_rust_bridge crate.

pub mod detect;
pub mod filter;
pub mod pdf;
