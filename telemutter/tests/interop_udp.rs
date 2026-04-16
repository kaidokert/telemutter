use std::io::{BufRead, BufReader};
use std::net::UdpSocket;
use std::process::{Command, Stdio};
use std::time::Duration;

use telemutter::{FrameWrite, Receiver, Sid, SidMode, write_frame};
mod common;
use common::crc32_ieee;

const X: usize = 16;
const S: usize = 2;

fn has_python() -> bool {
    Command::new("python")
        .arg("--version")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn schema_bytes() -> Vec<u8> {
    let body = b"interop-v1";
    let mut out = Vec::with_capacity(1 + body.len());
    out.push(0x40 | (body.len() as u8));
    out.extend_from_slice(body);
    out
}

fn epoch_frames(schema: &[u8], sid8: u8) -> Vec<[u8; X]> {
    let mut out = Vec::new();
    let payload_len = X - 1 - S;
    let payload_a = vec![0x11; payload_len];
    let payload_b = vec![0x22; payload_len];

    let mut f0 = [0u8; X];
    write_frame(FrameWrite {
        out_frame: &mut f0,
        schema_lane_bytes: S,
        schema_start: true,
        sid_mode: SidMode::Sid8,
        sid: Some(Sid::Sid8(sid8)),
        schema_chunk: &schema[..1],
        payload: &payload_a,
    })
    .unwrap();
    out.push(f0);

    let mut i = 1usize;
    while i < schema.len() {
        let j = (i + S).min(schema.len());
        let mut f = [0u8; X];
        write_frame(FrameWrite {
            out_frame: &mut f,
            schema_lane_bytes: S,
            schema_start: false,
            sid_mode: SidMode::Sid8,
            sid: None,
            schema_chunk: &schema[i..j],
            payload: &payload_b,
        })
        .unwrap();
        out.push(f);
        i = j;
    }
    out
}

#[test]
fn interop_python_sender_rust_receiver_udp() {
    if !has_python() {
        eprintln!("python not available; skipping interop test");
        return;
    }

    let schema = schema_bytes();
    let sid8 = (crc32_ieee(&schema) & 0xFF) as u8;

    let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
    rx.set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    let port = rx.local_addr().unwrap().port().to_string();

    let manifest = env!("CARGO_MANIFEST_DIR");
    let py_path = format!("{manifest}/../python");
    let script = format!("{manifest}/../python/scripts/udp_interop_peer.py");

    let status = Command::new("python")
        .arg(&script)
        .arg("send")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(&port)
        .arg("--loops")
        .arg("5")
        .env("PYTHONPATH", &py_path)
        .status()
        .unwrap();
    assert!(status.success());

    let mut r = Receiver::<256>::new(256, 512);
    let mut buf = [0u8; 128];
    let mut ok = false;
    for _ in 0..512 {
        let Ok((n, _)) = rx.recv_from(&mut buf) else {
            break;
        };
        if n != X {
            continue;
        }
        let _ = r.process_frame(&buf[..n], S);
        if r.schema_bytes() == Some(schema.as_slice()) {
            ok = true;
            break;
        }
    }
    assert!(ok);

    // Keep compiler from optimizing away.
    assert_eq!(sid8, (crc32_ieee(&schema) & 0xFF) as u8);
}

#[test]
fn interop_rust_sender_python_receiver_udp() {
    if !has_python() {
        eprintln!("python not available; skipping interop test");
        return;
    }

    let schema = schema_bytes();
    let sid8 = (crc32_ieee(&schema) & 0xFF) as u8;
    let frames = epoch_frames(&schema, sid8);

    let manifest = env!("CARGO_MANIFEST_DIR");
    let py_path = format!("{manifest}/../python");
    let script = format!("{manifest}/../python/scripts/udp_interop_peer.py");

    let mut child = Command::new("python")
        .arg(&script)
        .arg("recv")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg("0")
        .arg("--timeout-ms")
        .arg("4000")
        .env("PYTHONPATH", &py_path)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().expect("python recv stdout not piped");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .expect("failed to read recv handshake");
    assert!(n > 0, "python recv exited before emitting PORT handshake");
    let port: u16 = line
        .trim()
        .strip_prefix("PORT:")
        .expect("missing PORT: handshake from python recv")
        .parse()
        .expect("invalid PORT value from python recv");

    let tx = UdpSocket::bind("127.0.0.1:0").unwrap();
    for _ in 0..40 {
        for f in &frames {
            tx.send_to(f, ("127.0.0.1", port)).unwrap();
        }
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
    }

    let status = child.wait().unwrap();
    assert!(status.success());
}
