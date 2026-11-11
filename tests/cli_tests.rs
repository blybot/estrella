//! End-to-end CLI tests: run the binary against a local TCP "printer".

use std::io::Read;
use std::net::TcpListener;
use std::process::Command;
use std::thread;

const ESCPOS_PREAMBLE: [u8; 7] = [0x1B, 0x40, 0x1C, 0x2E, 0x1B, 0x74, 0x00];
const ESCPOS_PARTIAL_CUT: [u8; 4] = [0x1D, 0x56, 0x42, 0x00];

fn estrella() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_estrella"));
    for var in [
        "ESTRELLA_PROTOCOL",
        "ESTRELLA_ADDRESS",
        "ESTRELLA_DEVICE",
        "ESTRELLA_LISTEN",
    ] {
        cmd.env_remove(var);
    }
    cmd
}

/// Accept one connection and return everything sent on it.
fn fake_printer() -> (String, thread::JoinHandle<Vec<u8>>) {
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

fn assert_escpos_receipt(bytes: &[u8]) {
    assert!(bytes.starts_with(&ESCPOS_PREAMBLE), "missing ESC/POS init");
    assert!(bytes.ends_with(&ESCPOS_PARTIAL_CUT), "missing ESC/POS cut");
    // StarPRNT's ESC GS prefix must not appear in ESC/POS output.
    assert!(!bytes.windows(2).any(|w| w == [0x1B, 0x1D]));
}

#[test]
fn prints_escpos_over_tcp_configured_by_environment() {
    let (address, received) = fake_printer();

    let output = estrella()
        .args(["print", "receipt"])
        .env("ESTRELLA_PROTOCOL", "escpos")
        .env("ESTRELLA_ADDRESS", &address)
        .output()
        .unwrap();

    assert!(output.status.success(), "{:?}", output);
    assert_escpos_receipt(&received.join().unwrap());
}

#[test]
fn prints_escpos_over_tcp_configured_by_flags() {
    let (address, received) = fake_printer();

    let output = estrella()
        .args(["print", "receipt", "--protocol", "escpos", "--address"])
        .arg(&address)
        .output()
        .unwrap();

    assert!(output.status.success(), "{:?}", output);
    assert_escpos_receipt(&received.join().unwrap());
}

#[test]
fn flags_override_environment() {
    let (address, received) = fake_printer();

    let output = estrella()
        .args(["print", "receipt", "--protocol", "escpos"])
        .env("ESTRELLA_PROTOCOL", "star")
        .env("ESTRELLA_ADDRESS", &address)
        .output()
        .unwrap();

    assert!(output.status.success(), "{:?}", output);
    assert_escpos_receipt(&received.join().unwrap());
}

#[test]
fn address_and_device_are_mutually_exclusive() {
    let output = estrella()
        .args([
            "print",
            "receipt",
            "--address",
            "127.0.0.1",
            "--device",
            "/dev/null",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot be used with"), "{stderr}");
}

#[test]
fn rejects_unknown_protocol() {
    let output = estrella()
        .args(["print", "receipt", "--protocol", "zpl"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("escpos"), "{stderr}");
}
