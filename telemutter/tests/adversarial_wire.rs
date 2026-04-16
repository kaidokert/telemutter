use telemutter::{
    FrameError, FrameWrite, PROTOCOL_VERSION, Receiver, SCHEMA_PAD_BYTE, SchemaEvent, Sid, SidMode,
    Vft, parse_frame, write_frame,
};

const X: usize = 16;
const PAYLOAD_FILL: u8 = 0x5A;

mod common;
use common::crc32_ieee;

fn schema_bstr(body: &[u8]) -> Vec<u8> {
    let n = body.len();
    let mut out = Vec::with_capacity(n + 5);
    if n <= 23 {
        out.push(0x40 | (n as u8));
    } else if n <= 0xFF {
        out.push(0x58);
        out.push(n as u8);
    } else {
        out.push(0x59);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    }
    out.extend_from_slice(body);
    out
}

fn make_frame(
    schema_lane_bytes: usize,
    schema_start: bool,
    sid_mode: SidMode,
    sid: Option<Sid>,
    schema_chunk: &[u8],
) -> [u8; X] {
    let mut frame = [0u8; X];
    let payload_len = X - 1 - schema_lane_bytes;
    let payload = vec![PAYLOAD_FILL; payload_len];
    write_frame(FrameWrite {
        out_frame: &mut frame,
        schema_lane_bytes,
        schema_start,
        sid_mode,
        sid,
        schema_chunk,
        payload: &payload,
    })
    .expect("frame write");
    frame
}

fn make_schema_stream(
    schema_lane_bytes: usize,
    sid_mode: SidMode,
    sid: Sid,
    schema: &[u8],
) -> Vec<[u8; X]> {
    let sid_prefix = match sid_mode {
        SidMode::Sid8 => 1,
        SidMode::Sid32 => 4,
    };
    let first_cap = schema_lane_bytes - sid_prefix;
    let next_cap = schema_lane_bytes;

    let mut out = Vec::new();
    let first_take = schema.len().min(first_cap);
    out.push(make_frame(
        schema_lane_bytes,
        true,
        sid_mode,
        Some(sid),
        &schema[..first_take],
    ));

    let mut i = first_take;
    while i < schema.len() {
        let j = (i + next_cap).min(schema.len());
        out.push(make_frame(
            schema_lane_bytes,
            false,
            sid_mode,
            None,
            &schema[i..j],
        ));
        i = j;
    }
    out
}

#[test]
fn adversarial_garbage_and_truncation_never_panics() {
    let mut r = Receiver::<256>::new(256, 128);
    let mut seed: u32 = 0x1234_5678;
    for len in 0..64usize {
        let mut buf = vec![0u8; len];
        for b in &mut buf {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (seed >> 24) as u8;
        }
        let _ = parse_frame(&buf, 2, PROTOCOL_VERSION);
        let _ = r.process_frame(&buf, 2);
    }
}

#[test]
fn adversarial_offset_windows_from_corrupted_stream() {
    let schema = schema_bstr(b"hello-schema");
    let sid = Sid::Sid8((crc32_ieee(&schema) & 0xFF) as u8);
    let frames = make_schema_stream(2, SidMode::Sid8, sid, &schema);

    let mut blob = Vec::new();
    blob.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);
    for f in &frames {
        blob.extend_from_slice(f);
    }
    blob.extend_from_slice(&[0xFE, 0xED, 0xFA, 0xCE]);

    let mut r = Receiver::<256>::new(256, 128);
    for w in blob.windows(X) {
        let _ = r.process_frame(w, 2);
    }
}

#[test]
fn adversarial_drop_middle_schema_frame_requires_restart() {
    let schema = schema_bstr(b"schema-with-several-bytes");
    let sid = Sid::Sid8((crc32_ieee(&schema) & 0xFF) as u8);
    let frames = make_schema_stream(2, SidMode::Sid8, sid, &schema);
    assert!(frames.len() >= 3);

    let mut r = Receiver::<256>::new(256, 128);
    r.process_frame(&frames[0], 2).unwrap();
    // Drop one middle frame then continue.
    for f in frames.iter().skip(2) {
        let _ = r.process_frame(f, 2);
    }
    assert!(
        r.schema_bytes().is_none(),
        "should not install incomplete schema"
    );

    // A fresh restart should converge.
    for f in &frames {
        let _ = r.process_frame(f, 2).unwrap();
    }
    assert_eq!(r.schema_bytes(), Some(schema.as_slice()));
}

#[test]
fn adversarial_pathological_sampling_drops_every_start_frame() {
    let schema = schema_bstr(b"pathological-sampler");
    let sid = Sid::Sid8((crc32_ieee(&schema) & 0xFF) as u8);
    let epoch = make_schema_stream(2, SidMode::Sid8, sid, &schema);

    let mut wire = Vec::new();
    for _ in 0..20 {
        wire.extend_from_slice(&epoch);
    }

    let mut r = Receiver::<256>::new(256, 1024);
    for f in wire.iter().skip(1).step_by(epoch.len()) {
        // Keep sampling only non-start frames at same phase.
        let _ = r.process_frame(f, 2);
    }
    assert!(r.active_sid().is_none());
    assert!(r.schema_bytes().is_none());
}

#[test]
fn adversarial_crc_corruption_does_not_install_and_recovers_next_epoch() {
    let schema = schema_bstr(b"crc-adversarial");
    let sid32 = crc32_ieee(&schema);
    let frames = make_schema_stream(5, SidMode::Sid32, Sid::Sid32(sid32), &schema);

    let mut bad = frames.clone();
    // Flip one non-padding schema byte in the second frame if present.
    if bad.len() > 1 {
        bad[1][1] ^= 0x01;
    }

    let mut r = Receiver::<512>::new(512, 128);
    let mut saw_crc_err = false;
    for f in &bad {
        if let Err(FrameError::SchemaCrcMismatch) = r.process_frame(f, 5) {
            saw_crc_err = true;
        }
    }
    assert!(saw_crc_err);
    assert!(r.schema_bytes().is_none());

    // Next clean epoch installs.
    for f in &frames {
        let _ = r.process_frame(f, 5).unwrap();
    }
    assert_eq!(r.schema_bytes(), Some(schema.as_slice()));
}

#[test]
fn adversarial_new_start_aborts_previous_assembly() {
    let schema_a = schema_bstr(b"schema-a");
    let schema_b = schema_bstr(b"schema-b-newer");
    let sid_a = Sid::Sid8((crc32_ieee(&schema_a) & 0xFF) as u8);
    let sid_b = Sid::Sid8((crc32_ieee(&schema_b) & 0xFF) as u8);

    let frames_a = make_schema_stream(2, SidMode::Sid8, sid_a, &schema_a);
    let frames_b = make_schema_stream(2, SidMode::Sid8, sid_b, &schema_b);

    let mut r = Receiver::<256>::new(256, 128);
    r.process_frame(&frames_a[0], 2).unwrap();

    // Start a new schema before A completes.
    let evt = r.process_frame(&frames_b[0], 2).unwrap().event;
    assert!(matches!(evt, Some(SchemaEvent::Started { sid }) if sid == sid_b));

    for f in frames_b.iter().skip(1) {
        r.process_frame(f, 2).unwrap();
    }
    assert_eq!(r.active_sid(), Some(sid_b));
    assert_eq!(r.schema_bytes(), Some(schema_b.as_slice()));
}

#[test]
fn adversarial_schema_padding_after_completion_is_ignored() {
    let schema = schema_bstr(b"tiny");
    let sid = Sid::Sid8((crc32_ieee(&schema) & 0xFF) as u8);
    let frames = make_schema_stream(2, SidMode::Sid8, sid, &schema);
    let mut r = Receiver::<128>::new(128, 64);

    for f in &frames {
        r.process_frame(f, 2).unwrap();
    }
    assert_eq!(r.schema_bytes(), Some(schema.as_slice()));

    // Keep sending non-start frames with pure padding in schema lane.
    let mut pad = make_frame(2, false, SidMode::Sid8, None, &[]);
    pad[1] = SCHEMA_PAD_BYTE;
    pad[2] = SCHEMA_PAD_BYTE;
    for _ in 0..16 {
        r.process_frame(&pad, 2).unwrap();
    }
    assert_eq!(r.schema_bytes(), Some(schema.as_slice()));
}

#[test]
fn adversarial_invalid_vft_reserved_bits_rejected() {
    let mut frame = [0u8; X];
    frame[0] = Vft {
        version: PROTOCOL_VERSION,
        schema_start: false,
        sid_mode: SidMode::Sid8,
    }
    .encode()
    .unwrap()
        | 0x01; // reserved bit set
    frame[1..].fill(0xAA);

    let err = parse_frame(&frame, 2, PROTOCOL_VERSION).unwrap_err();
    assert_eq!(err, FrameError::ReservedBitsSet);
}

#[test]
fn adversarial_schema_size_boundary_thrash() {
    // max_schema_bytes=12 should accept 12 and reject 13 repeatedly without poisoning future runs.
    let mut r = Receiver::<32>::new(12, 64);

    let ok_schema = schema_bstr(&[0x11; 11]); // total = 12 (0x4B + 11)
    let ok_sid = Sid::Sid8((crc32_ieee(&ok_schema) & 0xFF) as u8);
    let ok_frames = make_schema_stream(2, SidMode::Sid8, ok_sid, &ok_schema);
    for f in &ok_frames {
        r.process_frame(f, 2).unwrap();
    }
    assert_eq!(r.schema_bytes(), Some(ok_schema.as_slice()));

    // Now push one byte too large (13 total), should error and clear assembly.
    let bad_schema = schema_bstr(&[0x22; 12]); // total = 13
    let bad_sid = Sid::Sid8((crc32_ieee(&bad_schema) & 0xFF) as u8);
    let bad_frames = make_schema_stream(2, SidMode::Sid8, bad_sid, &bad_schema);
    let mut saw_too_large = false;
    for f in &bad_frames {
        if let Err(FrameError::SchemaTooLarge) = r.process_frame(f, 2) {
            saw_too_large = true;
            break;
        }
    }
    assert!(saw_too_large);

    // Recover again with a valid-at-boundary schema.
    for f in &ok_frames {
        r.process_frame(f, 2).unwrap();
    }
    assert_eq!(r.schema_bytes(), Some(ok_schema.as_slice()));
}

#[test]
fn adversarial_schema_lane_mismatch_receiver_wrong_s() {
    // Sender uses S=2 (sid8 lane). Receiver sometimes parses with wrong S=5.
    let schema = schema_bstr(b"s-lane-mismatch");
    let sid = Sid::Sid8((crc32_ieee(&schema) & 0xFF) as u8);
    let frames = make_schema_stream(2, SidMode::Sid8, sid, &schema);

    let mut r = Receiver::<256>::new(256, 128);
    // Start frame with correct S.
    r.process_frame(&frames[0], 2).unwrap();
    // Misconfigured receiver call on a continuation frame may parse but corrupt assembly context.
    // Either way, it must not produce a completed wrong schema.
    let _ = r.process_frame(&frames[1], 5);
    assert_ne!(r.schema_bytes(), Some(schema.as_slice()));

    // Return to correct S and recover by waiting for next restart loop.
    for f in &frames {
        let _ = r.process_frame(f, 2);
    }
    assert_eq!(r.schema_bytes(), Some(schema.as_slice()));
}

#[test]
fn adversarial_vft_bitflip_schema_start_midstream() {
    // Bitflip sets schema_start on a non-start frame: receiver should reset assembly, not panic.
    let schema = schema_bstr(b"bitflip-schema-start");
    let sid = Sid::Sid8((crc32_ieee(&schema) & 0xFF) as u8);
    let mut frames = make_schema_stream(2, SidMode::Sid8, sid, &schema);
    assert!(frames.len() >= 2);

    // Corrupt frame #1 (non-start) to look like start but keep lane bytes unchanged.
    // This will interpret lane[0] as new sid and should derail current assembly.
    frames[1][0] |= 1 << 5; // schema_start bit

    let mut r = Receiver::<256>::new(256, 128);
    let _ = r.process_frame(&frames[0], 2).unwrap();
    let _ = r.process_frame(&frames[1], 2);
    // Assembly likely redirected to wrong SID; no completed schema expected.
    assert!(r.schema_bytes().is_none());

    // Next clean epoch recovers.
    let clean = make_schema_stream(2, SidMode::Sid8, sid, &schema);
    for f in &clean {
        r.process_frame(f, 2).unwrap();
    }
    assert_eq!(r.schema_bytes(), Some(schema.as_slice()));
}

#[test]
fn adversarial_vft_bitflip_sid_mode_on_start() {
    // Bitflip sid_mode on start frame should fail deterministically under S=2.
    let mut f = make_frame(2, true, SidMode::Sid8, Some(Sid::Sid8(0x42)), &[0x40]);
    f[0] |= 1 << 4; // sid_mode -> Sid32
    let err = parse_frame(&f, 2, PROTOCOL_VERSION).unwrap_err();
    assert_eq!(err, FrameError::InvalidSchemaLaneWidth);
}

#[test]
fn adversarial_old_schema_start_can_rollback_then_new_epoch_recovers() {
    // Lightweight replay-ish check: stale start frame can roll active SID back.
    let schema_old = schema_bstr(b"schema-old");
    let schema_new = schema_bstr(b"schema-new-current");
    let sid_old = Sid::Sid8((crc32_ieee(&schema_old) & 0xFF) as u8);
    let sid_new = Sid::Sid8((crc32_ieee(&schema_new) & 0xFF) as u8);

    let old_frames = make_schema_stream(2, SidMode::Sid8, sid_old, &schema_old);
    let new_frames = make_schema_stream(2, SidMode::Sid8, sid_new, &schema_new);

    let mut r = Receiver::<256>::new(256, 128);
    for f in &new_frames {
        r.process_frame(f, 2).unwrap();
    }
    assert_eq!(r.active_sid(), Some(sid_new));
    assert_eq!(r.schema_bytes(), Some(schema_new.as_slice()));

    // Inject only stale start frame (simulated replay/noise); this may change active_sid.
    r.process_frame(&old_frames[0], 2).unwrap();
    assert_eq!(r.active_sid(), Some(sid_old));

    // Fresh full current epoch restores correct schema/SID.
    for f in &new_frames {
        r.process_frame(f, 2).unwrap();
    }
    assert_eq!(r.active_sid(), Some(sid_new));
    assert_eq!(r.schema_bytes(), Some(schema_new.as_slice()));
}
