//! # Printer Protocol
//!
//! Which command language the printer speaks. The IR is protocol-neutral;
//! the protocol picks the code generator.

use super::PrinterConfig;

/// Printer command protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Protocol {
    /// Star Micronics StarPRNT (e.g. TSP650II)
    #[default]
    Star,
    /// Epson ESC/POS (e.g. Epson TM series, Xprinter XP-80T)
    #[value(name = "escpos")]
    EscPos,
}

impl Protocol {
    /// Hardware profile used when generating bytes for this protocol.
    pub fn config(self) -> PrinterConfig {
        match self {
            Protocol::Star => PrinterConfig::TSP650II,
            Protocol::EscPos => PrinterConfig::ESCPOS_80MM,
        }
    }
}
