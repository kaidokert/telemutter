#![no_std]

pub use telemutter::*;

const FRAME_LEN: usize = 8;
const SCHEMA_LANE_BYTES: usize = 2;

/// Performs one sid8 write+parse roundtrip.
/// Returns 0 on success, non-zero on envelope/protocol error.
pub fn roundtrip_status_sid8() -> u8 {
    let mut frame = [0u8; FRAME_LEN];
    let payload = [0xA1, 0xB2, 0xC3, 0xD4, 0xE5];
    let schema_chunk = [0x40]; // empty CBOR bstr

    let write = write_frame(FrameWrite {
        out_frame: &mut frame,
        schema_lane_bytes: SCHEMA_LANE_BYTES,
        schema_start: true,
        sid_mode: SidMode::Sid8,
        sid: Some(Sid::Sid8(0x42)),
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
