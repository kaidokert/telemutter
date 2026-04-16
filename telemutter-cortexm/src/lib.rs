#![no_std]

pub use telemutter::*;

const FRAME_LEN: usize = 16;
const SCHEMA_LANE_BYTES: usize = 5;

/// Performs one sid32 write+parse roundtrip.
/// Returns 0 on success, non-zero on envelope/protocol error.
pub fn roundtrip_status_sid32() -> u8 {
    let mut frame = [0u8; FRAME_LEN];
    let payload = [0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90, 0xA0];
    let schema_chunk = [0x40]; // empty CBOR bstr

    let write = write_frame(FrameWrite {
        out_frame: &mut frame,
        schema_lane_bytes: SCHEMA_LANE_BYTES,
        schema_start: true,
        sid_mode: SidMode::Sid32,
        sid: Some(Sid::Sid32(0x1234_5678)),
        schema_chunk: &schema_chunk,
        payload: &payload,
    });
    if write.is_err() {
        return 1;
    }

    match parse_frame(&frame, SCHEMA_LANE_BYTES, PROTOCOL_VERSION) {
        Ok(parsed) if parsed.payload == payload && parsed.schema_chunk == schema_chunk => 0,
        Ok(_) => 2,
        Err(_) => 3,
    }
}
