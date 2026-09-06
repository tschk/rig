//! rig — cross-lang native dependency manager.

pub mod cli;
pub mod detect;
pub mod expose;
pub mod manifest;
pub mod ops;
pub mod resolve;
pub mod util;

pub use detect::{DetectedHost, Language, detect_host};
pub use manifest::{Lockfile, Manifest};
