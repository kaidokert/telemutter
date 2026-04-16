use std::net::UdpSocket;
use std::time::Duration;

use telemutter::{FrameWrite, PROTOCOL_VERSION, Receiver, Sid, SidMode, parse_frame, write_frame};
mod common;
use common::crc32_ieee;

const X: usize = 16;
const S: usize = 2;

fn schema_bstr(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 2);
    if body.len() <= 23 {
        out.push(0x40 | (body.len() as u8));
    } else {
        out.push(0x58);
        out.push(body.len() as u8);
    }
    out.extend_from_slice(body);
    out
}

fn mk_frame(
    schema_start: bool,
    sid: Option<Sid>,
    schema_chunk: &[u8],
    payload_fill: u8,
) -> [u8; X] {
    let mut frame = [0u8; X];
    let payload = [payload_fill; X - 1 - S];
    write_frame(FrameWrite {
        out_frame: &mut frame,
        schema_lane_bytes: S,
        schema_start,
        sid_mode: SidMode::Sid8,
        sid,
        schema_chunk,
        payload: &payload,
    })
    .unwrap();
    frame
}

fn mk_schema_epoch(schema: &[u8], sid: Sid) -> Vec<[u8; X]> {
    let first_cap = S - 1;
    let mut out = Vec::new();
    let first_take = schema.len().min(first_cap);
    out.push(mk_frame(true, Some(sid), &schema[..first_take], 0x11));
    let mut i = first_take;
    while i < schema.len() {
        let j = (i + S).min(schema.len());
        out.push(mk_frame(false, None, &schema[i..j], 0x22));
        i = j;
    }
    out
}

#[test]
fn udp_loopback_converges_schema() {
    let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
    rx.set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let tx = UdpSocket::bind("127.0.0.1:0").unwrap();
    let rx_addr = rx.local_addr().unwrap();

    let schema = schema_bstr(b"udp-schema");
    let sid8 = (crc32_ieee(&schema) & 0xFF) as u8;
    let epoch = mk_schema_epoch(&schema, Sid::Sid8(sid8));

    for _ in 0..4 {
        for f in &epoch {
            tx.send_to(f, rx_addr).unwrap();
        }
    }

    let mut r = Receiver::<256>::new(256, 256);
    let mut got = false;
    let mut buf = [0u8; 64];
    for _ in 0..(epoch.len() * 6) {
        if let Ok((n, _)) = rx.recv_from(&mut buf) {
            if n != X {
                continue;
            }
            if r.process_frame(&buf[..n], S).is_ok() && r.schema_bytes() == Some(schema.as_slice())
            {
                got = true;
                break;
            }
        } else {
            break;
        }
    }
    assert!(got, "receiver should converge over UDP loopback");
}

#[test]
fn udp_loopback_with_garbage_and_truncation_still_converges() {
    let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
    rx.set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let tx = UdpSocket::bind("127.0.0.1:0").unwrap();
    let rx_addr = rx.local_addr().unwrap();

    let schema = schema_bstr(b"udp-adversarial-schema");
    let sid8 = (crc32_ieee(&schema) & 0xFF) as u8;
    let epoch = mk_schema_epoch(&schema, Sid::Sid8(sid8));

    // Interleave valid frames with transport garbage.
    for (i, f) in epoch.iter().enumerate() {
        tx.send_to(&[0xDE, 0xAD, 0xBE], rx_addr).unwrap(); // truncated
        let mut corrupt = *f;
        if i % 3 == 0 {
            corrupt[0] ^= 0x40; // wrong version bit pattern sometimes
            tx.send_to(&corrupt, rx_addr).unwrap();
        }
        tx.send_to(f, rx_addr).unwrap(); // valid frame
        tx.send_to(&[0u8; X + 5], rx_addr).unwrap(); // oversize datagram
    }

    // Send a clean epoch for guaranteed convergence.
    for f in &epoch {
        tx.send_to(f, rx_addr).unwrap();
    }

    let mut r = Receiver::<512>::new(512, 512);
    let mut buf = [0u8; 128];
    let mut got = false;
    for _ in 0..512 {
        let Ok((n, _)) = rx.recv_from(&mut buf) else {
            break;
        };
        if n != X {
            continue;
        }
        // Channel framing layer: only fixed-size frames are fed to envelope parser.
        if parse_frame(&buf[..n], S, PROTOCOL_VERSION).is_err() {
            continue;
        }
        let _ = r.process_frame(&buf[..n], S);
        if r.schema_bytes() == Some(schema.as_slice()) {
            got = true;
            break;
        }
    }
    assert!(got, "receiver should converge despite garbage datagrams");
}
