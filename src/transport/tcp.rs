//! # TCP Printer Transport
//!
//! Raw TCP printing ("JetDirect" / port 9100), as used by networked ESC/POS
//! printers such as the Xprinter XP-80T. The printer reads the byte stream
//! straight off the socket, so TCP flow control paces large jobs.

use std::io::Write;
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::thread;
use std::time::Duration;

use crate::error::EstrellaError;

/// Default raw printing port.
pub const DEFAULT_PORT: u16 = 9100;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// Pause between independent print jobs, giving the printer time to finish
/// the current job before the next Init arrives.
const JOB_DELAY: Duration = Duration::from_millis(2000);

pub struct TcpTransport {
    stream: TcpStream,
}

impl TcpTransport {
    /// Connect to a printer at `host` or `host:port` (port defaults to 9100).
    pub fn connect(address: &str) -> Result<Self, EstrellaError> {
        let with_port = with_default_port(address);
        let addrs = with_port
            .to_socket_addrs()
            .map_err(|e| EstrellaError::Transport(format!("Failed to resolve {with_port}: {e}")))?;

        let mut last_error = None;
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
                Ok(stream) => {
                    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
                    stream.set_nodelay(true)?;
                    return Ok(Self { stream });
                }
                Err(e) => last_error = Some(e),
            }
        }
        Err(EstrellaError::Transport(match last_error {
            Some(e) => format!("Failed to connect to {with_port}: {e}"),
            None => format!("No addresses found for {with_port}"),
        }))
    }

    pub fn write_all(&mut self, data: &[u8]) -> Result<(), EstrellaError> {
        self.stream
            .write_all(data)
            .and_then(|_| self.stream.flush())
            .map_err(|e| EstrellaError::Transport(format!("Write failed: {e}")))
    }

    /// Send independent print jobs with a pause between them.
    pub fn send_jobs(&mut self, jobs: &[Vec<u8>]) -> Result<(), EstrellaError> {
        for (i, job) in jobs.iter().enumerate() {
            self.write_all(job)?;
            if i + 1 < jobs.len() {
                thread::sleep(JOB_DELAY);
            }
        }
        Ok(())
    }
}

/// Append the default port unless `address` already has one.
fn with_default_port(address: &str) -> String {
    if address.parse::<SocketAddr>().is_ok() {
        return address.to_string();
    }
    match address.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') && port.parse::<u16>().is_ok() => {
            address.to_string()
        }
        // Bare IPv6 address
        Some(_) if !address.starts_with('[') => format!("[{address}]:{DEFAULT_PORT}"),
        _ => format!("{address}:{DEFAULT_PORT}"),
    }
}

impl Drop for TcpTransport {
    fn drop(&mut self) {
        // Half-close so the printer sees end-of-job; buffered data is still delivered.
        let _ = self.stream.shutdown(Shutdown::Write);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    fn capture_listener() -> (String, thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut received = Vec::new();
            socket.read_to_end(&mut received).unwrap();
            received
        });
        (address, handle)
    }

    #[test]
    fn sends_bytes_to_the_printer_and_closes() {
        let (address, received) = capture_listener();

        let mut transport = TcpTransport::connect(&address).unwrap();
        transport.write_all(&[0x1B, 0x40, b'h', b'i']).unwrap();
        drop(transport);

        assert_eq!(received.join().unwrap(), vec![0x1B, 0x40, b'h', b'i']);
    }

    #[test]
    fn large_payloads_arrive_intact() {
        let (address, received) = capture_listener();
        let payload: Vec<u8> = (0..500_000u32).map(|i| (i % 251) as u8).collect();

        let mut transport = TcpTransport::connect(&address).unwrap();
        transport.write_all(&payload).unwrap();
        drop(transport);

        assert_eq!(received.join().unwrap(), payload);
    }

    #[test]
    fn port_defaults_to_9100() {
        assert_eq!(with_default_port("192.168.1.50"), "192.168.1.50:9100");
        assert_eq!(with_default_port("printer.lan"), "printer.lan:9100");
        assert_eq!(with_default_port("printer.lan:9101"), "printer.lan:9101");
        assert_eq!(with_default_port("10.0.0.5:9100"), "10.0.0.5:9100");
        assert_eq!(with_default_port("fd00::5"), "[fd00::5]:9100");
        assert_eq!(with_default_port("[fd00::5]:9101"), "[fd00::5]:9101");
    }

    #[test]
    fn connection_refused_is_a_transport_error() {
        // Bind then drop to get a port with nothing listening.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let result = TcpTransport::connect(&format!("127.0.0.1:{port}"));
        assert!(matches!(result, Err(EstrellaError::Transport(_))));
    }
}
