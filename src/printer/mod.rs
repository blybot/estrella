//! # Printer Module
//!
//! This module provides printer-specific configurations and utilities.
//!
//! ## Modules
//!
//! - [`config`]: Printer hardware specifications
//! - [`protocol`]: Printer command protocol (StarPRNT or ESC/POS)

pub mod config;
pub mod protocol;

pub use config::PrinterConfig;
pub use protocol::Protocol;
