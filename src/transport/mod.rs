//! # Printer Transport Layer
//!
//! This module provides communication backends for sending data to printers.
//!
//! ## Available Transports
//!
//! - [`bluetooth`]: Bluetooth RFCOMM for wireless printing (Linux)
//! - [`tcp`]: Raw TCP (port 9100) for networked printers
//!
//! [`Printer`] ties a transport target to a [`Protocol`], so callers hand it
//! IR programs and it encodes and sends them.

pub mod bluetooth;
pub mod tcp;

pub use bluetooth::BluetoothTransport;
pub use tcp::TcpTransport;

use crate::error::EstrellaError;
use crate::ir::Program;
use crate::printer::Protocol;

/// Where the printer is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrinterTarget {
    /// Character device, e.g. a Bluetooth RFCOMM TTY (`/dev/rfcomm0`).
    Device(String),
    /// Network printer, `host` or `host:port` (port defaults to 9100).
    Tcp(String),
}

impl std::fmt::Display for PrinterTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrinterTarget::Device(path) => write!(f, "{path}"),
            PrinterTarget::Tcp(address) => write!(f, "tcp://{address}"),
        }
    }
}

/// An open connection to a printer.
pub trait PrinterConnection {
    fn write_all(&mut self, data: &[u8]) -> Result<(), EstrellaError>;

    /// Send independent print jobs with a pause between them.
    fn send_jobs(&mut self, jobs: &[Vec<u8>]) -> Result<(), EstrellaError>;
}

impl PrinterConnection for BluetoothTransport {
    fn write_all(&mut self, data: &[u8]) -> Result<(), EstrellaError> {
        BluetoothTransport::write_all(self, data)
    }

    fn send_jobs(&mut self, jobs: &[Vec<u8>]) -> Result<(), EstrellaError> {
        BluetoothTransport::send_jobs(self, jobs)
    }
}

impl PrinterConnection for TcpTransport {
    fn write_all(&mut self, data: &[u8]) -> Result<(), EstrellaError> {
        TcpTransport::write_all(self, data)
    }

    fn send_jobs(&mut self, jobs: &[Vec<u8>]) -> Result<(), EstrellaError> {
        TcpTransport::send_jobs(self, jobs)
    }
}

impl PrinterTarget {
    pub fn open(&self) -> Result<Box<dyn PrinterConnection>, EstrellaError> {
        Ok(match self {
            PrinterTarget::Device(path) => Box::new(BluetoothTransport::open(path)?),
            PrinterTarget::Tcp(address) => Box::new(TcpTransport::connect(address)?),
        })
    }
}

/// A printer: where it is and which protocol it speaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printer {
    pub protocol: Protocol,
    pub target: PrinterTarget,
}

impl Printer {
    /// Encode and print a single program.
    pub fn print_program(&self, program: &Program) -> Result<(), EstrellaError> {
        self.print_bytes(&program.encode(self.protocol)?)
    }

    /// Encode and print programs as independent jobs (see
    /// [`Program::split_for_long_print`]).
    pub fn print_programs(&self, programs: &[Program]) -> Result<(), EstrellaError> {
        let jobs = programs
            .iter()
            .map(|p| p.encode(self.protocol))
            .collect::<Result<Vec<_>, _>>()?;
        self.target.open()?.send_jobs(&jobs)
    }

    /// Send bytes already encoded for this printer's protocol.
    pub fn print_bytes(&self, data: &[u8]) -> Result<(), EstrellaError> {
        self.target.open()?.write_all(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Op;
    use std::io::Read;
    use std::net::TcpListener;

    #[test]
    fn escpos_printer_encodes_programs_and_sends_them_over_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let printer = Printer {
            protocol: Protocol::EscPos,
            target: PrinterTarget::Tcp(listener.local_addr().unwrap().to_string()),
        };
        let receiver = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut received = Vec::new();
            socket.read_to_end(&mut received).unwrap();
            received
        });

        let program = Program {
            ops: vec![Op::Init, Op::Text("hi".into()), Op::Cut { partial: true }],
        };
        printer.print_programs(&[program]).unwrap();

        let expected: Vec<u8> = [
            &[0x1B, 0x40, 0x1C, 0x2E, 0x1B, 0x74, 0x00][..],
            b"hi",
            &[0x1D, 0x56, 0x42, 0x00],
        ]
        .concat();
        assert_eq!(receiver.join().unwrap(), expected);
    }

    #[test]
    fn encoding_errors_are_reported_before_connecting() {
        let printer = Printer {
            protocol: Protocol::EscPos,
            // Nothing listens here; an encoding error must win.
            target: PrinterTarget::Tcp("127.0.0.1:1".into()),
        };
        let program = Program {
            ops: vec![Op::NvDelete { key: "A0".into() }],
        };
        let err = printer.print_programs(&[program]).unwrap_err();
        assert!(matches!(err, EstrellaError::InvalidCommand(_)));
    }
}
