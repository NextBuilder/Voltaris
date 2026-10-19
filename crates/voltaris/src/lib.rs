//! Voltaris — cdylib entry point for KiCad plugin integration.
//!
//! When compiled as a cdylib, this crate exposes a C-compatible ABI that
//! a thin Python action plugin (or KiCad PCM package) can load.

mod config;
mod ffi;
mod transport;

pub use ffi::*;
