#![no_std]

use crc_any::CRCu32;

/// Protocol version encoded in `VFT.b7..b6`.
pub const PROTOCOL_VERSION: u8 = 0;

/// Sentinel used for schema-lane padding once schema bytes are exhausted.
pub const SCHEMA_PAD_BYTE: u8 = 0xA5;

/// SID encoding mode selected by `VFT.b4`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SidMode {
    Sid8,
    Sid32,
}

/// SID value carried on `schema_start` frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Sid {
    Sid8(u8),
    Sid32(u32),
}

/// Parsed VFT fields from the first byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Vft {
    pub version: u8,
    pub schema_start: bool,
    pub sid_mode: SidMode,
}

impl Vft {
    pub fn parse(byte: u8) -> Result<Self, FrameError> {
        let version = (byte >> 6) & 0b11;
        let schema_start = ((byte >> 5) & 0b1) != 0;
        let sid_mode = if ((byte >> 4) & 0b1) != 0 {
            SidMode::Sid32
        } else {
            SidMode::Sid8
        };
        let reserved = byte & 0x0F;
        if reserved != 0 {
            return Err(FrameError::ReservedBitsSet);
        }
        Ok(Self {
            version,
            schema_start,
            sid_mode,
        })
    }

    pub fn encode(self) -> Result<u8, FrameError> {
        if self.version > 0b11 {
            return Err(FrameError::InvalidVersion);
        }
        let mut out = (self.version & 0b11) << 6;
        if self.schema_start {
            out |= 1 << 5;
        }
        if self.sid_mode == SidMode::Sid32 {
            out |= 1 << 4;
        }
        Ok(out)
    }
}

/// Errors returned while parsing frames and evolving receiver state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    InvalidVersion,
    ReservedBitsSet,
    WrongProtocolVersion,
    FrameTooShort,
    InvalidSchemaLaneWidth,
    InvalidPayloadLength,
    InvalidCborLengthPrefix,
    SchemaTooLarge,
    SchemaFrameBudgetExceeded,
    SidMismatch,
    SchemaCrcMismatch,
    MissingSidOnStart,
    UnexpectedSidOnNonStart,
    SidModeMismatch,
    SchemaChunkTooLong,
    PayloadLenMismatch,
}

/// Optional host-side diagnostic payload for envelope errors.
///
/// Enabled with `telemutter/diagnostic-errors`.
#[cfg(feature = "diagnostic-errors")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameErrorDetail {
    pub kind: FrameError,
    pub expected: Option<usize>,
    pub actual: Option<usize>,
}

/// Parsed frame view. All slices borrow directly from the input frame.
#[derive(Debug)]
pub struct ParsedFrame<'a> {
    pub vft: Vft,
    pub sid: Option<Sid>,
    pub schema_chunk: &'a [u8],
    pub payload: &'a [u8],
}

/// Inputs for writing one fixed-size frame.
pub struct FrameWrite<'a> {
    pub out_frame: &'a mut [u8],
    pub schema_lane_bytes: usize,
    pub schema_start: bool,
    pub sid_mode: SidMode,
    pub sid: Option<Sid>,
    pub schema_chunk: &'a [u8],
    pub payload: &'a [u8],
}

/// Parse one fixed-size frame according to the v1 minimal envelope.
///
/// `schema_lane_bytes` is deployment/epoch configuration (`S`).
pub fn parse_frame<'a>(
    frame: &'a [u8],
    schema_lane_bytes: usize,
    expected_version: u8,
) -> Result<ParsedFrame<'a>, FrameError> {
    if frame.len() < 2 {
        return Err(FrameError::FrameTooShort);
    }
    let vft = Vft::parse(frame[0])?;
    if vft.version != expected_version {
        return Err(FrameError::WrongProtocolVersion);
    }

    let sid_prefix_len = match vft.sid_mode {
        SidMode::Sid8 => 1,
        SidMode::Sid32 => 4,
    };
    let min_s = sid_prefix_len + 1;
    if schema_lane_bytes < min_s {
        return Err(FrameError::InvalidSchemaLaneWidth);
    }

    let header_len = 1usize;
    let payload_len = frame
        .len()
        .checked_sub(header_len + schema_lane_bytes)
        .ok_or(FrameError::FrameTooShort)?;
    if payload_len == 0 {
        return Err(FrameError::InvalidPayloadLength);
    }

    let lane = &frame[header_len..header_len + schema_lane_bytes];
    let payload = &frame[header_len + schema_lane_bytes..];
    let (sid, schema_chunk) = if vft.schema_start {
        match vft.sid_mode {
            SidMode::Sid8 => (Some(Sid::Sid8(lane[0])), &lane[1..]),
            SidMode::Sid32 => {
                let sid = u32::from_le_bytes([lane[0], lane[1], lane[2], lane[3]]);
                (Some(Sid::Sid32(sid)), &lane[4..])
            }
        }
    } else {
        (None, lane)
    };

    Ok(ParsedFrame {
        vft,
        sid,
        schema_chunk,
        payload,
    })
}

/// Diagnostic variant of [`parse_frame`] with optional expected/actual size context.
#[cfg(feature = "diagnostic-errors")]
pub fn parse_frame_detailed<'a>(
    frame: &'a [u8],
    schema_lane_bytes: usize,
    expected_version: u8,
) -> Result<ParsedFrame<'a>, FrameErrorDetail> {
    parse_frame(frame, schema_lane_bytes, expected_version).map_err(|kind| match kind {
        FrameError::FrameTooShort => FrameErrorDetail {
            kind,
            expected: Some(2.max(1 + schema_lane_bytes + 1)),
            actual: Some(frame.len()),
        },
        FrameError::WrongProtocolVersion => FrameErrorDetail {
            kind,
            expected: Some(expected_version as usize),
            actual: frame.first().map(|b| ((b >> 6) & 0b11) as usize),
        },
        FrameError::InvalidSchemaLaneWidth => {
            let sid_mode = frame
                .first()
                .and_then(|b| Vft::parse(*b).ok())
                .map(|v| v.sid_mode)
                .unwrap_or(SidMode::Sid8);
            let min_s = match sid_mode {
                SidMode::Sid8 => 2,
                SidMode::Sid32 => 5,
            };
            FrameErrorDetail {
                kind,
                expected: Some(min_s),
                actual: Some(schema_lane_bytes),
            }
        }
        FrameError::InvalidPayloadLength => FrameErrorDetail {
            kind,
            expected: Some(1),
            actual: frame.len().checked_sub(1 + schema_lane_bytes),
        },
        _ => FrameErrorDetail {
            kind,
            expected: None,
            actual: None,
        },
    })
}

/// Write one frame into `out_frame` using the same envelope rules as `parse_frame`.
///
/// - On `schema_start=1`, SID is prefixed inside schema lane.
/// - Unused schema lane bytes are padded with `SCHEMA_PAD_BYTE`.
pub fn write_frame(args: FrameWrite<'_>) -> Result<(), FrameError> {
    let sid_prefix_len = match args.sid_mode {
        SidMode::Sid8 => 1,
        SidMode::Sid32 => 4,
    };
    let min_s = sid_prefix_len + 1;
    if args.schema_lane_bytes < min_s {
        return Err(FrameError::InvalidSchemaLaneWidth);
    }
    if args.out_frame.len() < 2 {
        return Err(FrameError::FrameTooShort);
    }

    let payload_len = args
        .out_frame
        .len()
        .checked_sub(1 + args.schema_lane_bytes)
        .ok_or(FrameError::FrameTooShort)?;
    if payload_len == 0 {
        return Err(FrameError::InvalidPayloadLength);
    }
    if args.payload.len() != payload_len {
        return Err(FrameError::PayloadLenMismatch);
    }

    if args.schema_start && args.sid.is_none() {
        return Err(FrameError::MissingSidOnStart);
    }
    if !args.schema_start && args.sid.is_some() {
        return Err(FrameError::UnexpectedSidOnNonStart);
    }

    // Validate SID variant matches sid_mode when provided.
    if let Some(sid) = args.sid {
        match (args.sid_mode, sid) {
            (SidMode::Sid8, Sid::Sid8(_)) | (SidMode::Sid32, Sid::Sid32(_)) => {}
            _ => return Err(FrameError::SidModeMismatch),
        }
    }

    let schema_cap = if args.schema_start {
        args.schema_lane_bytes - sid_prefix_len
    } else {
        args.schema_lane_bytes
    };
    if args.schema_chunk.len() > schema_cap {
        return Err(FrameError::SchemaChunkTooLong);
    }

    let vft = Vft {
        version: PROTOCOL_VERSION,
        schema_start: args.schema_start,
        sid_mode: args.sid_mode,
    }
    .encode()?;
    args.out_frame[0] = vft;

    // Fill lane with padding first.
    let lane_start = 1usize;
    let lane_end = lane_start + args.schema_lane_bytes;
    for b in &mut args.out_frame[lane_start..lane_end] {
        *b = SCHEMA_PAD_BYTE;
    }

    // SID prefix for start frames.
    let mut cursor = lane_start;
    if args.schema_start {
        match args.sid.expect("checked above") {
            Sid::Sid8(v) => {
                args.out_frame[cursor] = v;
                cursor += 1;
            }
            Sid::Sid32(v) => {
                let sid = v.to_le_bytes();
                args.out_frame[cursor..cursor + 4].copy_from_slice(&sid);
                cursor += 4;
            }
        }
    }

    // Schema chunk.
    let end = cursor + args.schema_chunk.len();
    args.out_frame[cursor..end].copy_from_slice(args.schema_chunk);

    // Payload.
    args.out_frame[lane_end..].copy_from_slice(args.payload);
    Ok(())
}

/// Diagnostic variant of [`write_frame`] with expected/actual size context.
#[cfg(feature = "diagnostic-errors")]
pub fn write_frame_detailed(args: FrameWrite<'_>) -> Result<(), FrameErrorDetail> {
    let sid_prefix_len = match args.sid_mode {
        SidMode::Sid8 => 1,
        SidMode::Sid32 => 4,
    };
    let schema_cap = if args.schema_start {
        args.schema_lane_bytes.saturating_sub(sid_prefix_len)
    } else {
        args.schema_lane_bytes
    };
    let schema_lane_actual = args.schema_lane_bytes;
    let payload_expected = args
        .out_frame
        .len()
        .saturating_sub(1 + args.schema_lane_bytes);
    let payload_actual = args.payload.len();
    let schema_chunk_actual = args.schema_chunk.len();

    write_frame(args).map_err(|kind| match kind {
        FrameError::InvalidSchemaLaneWidth => FrameErrorDetail {
            kind,
            expected: Some(sid_prefix_len + 1),
            actual: Some(schema_lane_actual),
        },
        FrameError::FrameTooShort => FrameErrorDetail {
            kind,
            expected: Some(2),
            actual: None,
        },
        FrameError::InvalidPayloadLength => FrameErrorDetail {
            kind,
            expected: Some(1),
            actual: Some(payload_expected),
        },
        FrameError::PayloadLenMismatch => FrameErrorDetail {
            kind,
            expected: Some(payload_expected),
            actual: Some(payload_actual),
        },
        FrameError::SchemaChunkTooLong => FrameErrorDetail {
            kind,
            expected: Some(schema_cap),
            actual: Some(schema_chunk_actual),
        },
        _ => FrameErrorDetail {
            kind,
            expected: None,
            actual: None,
        },
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaEvent {
    Started {
        sid: Sid,
    },
    Progress {
        sid: Sid,
        received: usize,
        expected_total: Option<usize>,
    },
    Complete {
        sid: Sid,
        schema_len: usize,
    },
}

#[derive(Debug)]
pub struct ProcessedFrame<'a> {
    pub payload: &'a [u8],
    pub event: Option<SchemaEvent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Assembly {
    sid: Sid,
    received: usize,
    expected_total: Option<usize>,
    frames_seen: usize,
}

/// `no_std` receiver that reassembles schema stream bytes in a caller-sized buffer.
pub struct Receiver<const N: usize> {
    schema_buf: [u8; N],
    assembly: Option<Assembly>,
    active_sid: Option<Sid>,
    max_schema_bytes: usize,
    max_schema_frames: usize,
}

impl<const N: usize> Receiver<N> {
    pub const fn new(max_schema_bytes: usize, max_schema_frames: usize) -> Self {
        Self {
            schema_buf: [0u8; N],
            assembly: None,
            active_sid: None,
            max_schema_bytes,
            max_schema_frames,
        }
    }

    pub fn active_sid(&self) -> Option<Sid> {
        self.active_sid
    }

    pub fn schema_bytes(&self) -> Option<&[u8]> {
        self.assembly
            .filter(|a| a.expected_total == Some(a.received))
            .map(|a| &self.schema_buf[..a.received])
    }

    pub fn process_frame<'a>(
        &mut self,
        frame: &'a [u8],
        schema_lane_bytes: usize,
    ) -> Result<ProcessedFrame<'a>, FrameError> {
        let parsed = parse_frame(frame, schema_lane_bytes, PROTOCOL_VERSION)?;
        if parsed.vft.schema_start {
            let sid = parsed.sid.expect("sid must exist on schema_start");
            self.assembly = Some(Assembly {
                sid,
                received: 0,
                expected_total: None,
                frames_seen: 0,
            });
            self.active_sid = Some(sid);
            self.append_schema_chunk(parsed.schema_chunk)?;
            let event = match self.progress_event() {
                Some(SchemaEvent::Complete { .. }) => self.progress_event(),
                _ => Some(SchemaEvent::Started { sid }),
            };
            return Ok(ProcessedFrame {
                payload: parsed.payload,
                event,
            });
        }

        self.append_schema_chunk(parsed.schema_chunk)?;
        let event = self.progress_event();
        Ok(ProcessedFrame {
            payload: parsed.payload,
            event,
        })
    }

    fn abort_assembly(&mut self) {
        self.assembly = None;
        self.active_sid = None;
    }

    fn append_schema_chunk(&mut self, chunk: &[u8]) -> Result<(), FrameError> {
        let mut asm = match self.assembly {
            Some(a) => a,
            None => return Ok(()),
        };

        // Once complete, ignore subsequent schema-lane bytes (typically padding) with zero work.
        if let Some(total) = asm.expected_total
            && asm.received >= total
        {
            self.assembly = Some(asm);
            return Ok(());
        }

        asm.frames_seen += 1;
        if asm.frames_seen > self.max_schema_frames {
            self.abort_assembly();
            return Err(FrameError::SchemaFrameBudgetExceeded);
        }

        // Prefix-discovery phase: copy byte-by-byte until CBOR bstr total length is known.
        let mut chunk_off = 0usize;
        while chunk_off < chunk.len() && asm.expected_total.is_none() {
            if asm.received >= self.max_schema_bytes || asm.received >= N {
                self.abort_assembly();
                return Err(FrameError::SchemaTooLarge);
            }
            self.schema_buf[asm.received] = chunk[chunk_off];
            asm.received += 1;
            chunk_off += 1;

            asm.expected_total = match cbor_bstr_total_len(&self.schema_buf[..asm.received]) {
                Ok(v) => v,
                Err(e) => {
                    self.abort_assembly();
                    return Err(e);
                }
            };
            if let Some(total) = asm.expected_total
                && (total > self.max_schema_bytes || total > N)
            {
                self.abort_assembly();
                return Err(FrameError::SchemaTooLarge);
            }
        }

        // Bulk-copy phase once total length is known.
        if let Some(total) = asm.expected_total
            && chunk_off < chunk.len()
            && asm.received < total
        {
            let remaining = total - asm.received;
            let take = remaining.min(chunk.len() - chunk_off);
            self.schema_buf[asm.received..asm.received + take]
                .copy_from_slice(&chunk[chunk_off..chunk_off + take]);
            asm.received += take;
        }

        if let Some(total) = asm.expected_total
            && asm.received == total
        {
            let schema = &self.schema_buf[..total];
            match asm.sid {
                Sid::Sid32(expected) => {
                    let got = crc32_ieee(schema);
                    if got != expected {
                        self.abort_assembly();
                        return Err(FrameError::SchemaCrcMismatch);
                    }
                }
                Sid::Sid8(expected) => {
                    let got = (crc32_ieee(schema) & 0xFF) as u8;
                    if got != expected {
                        self.abort_assembly();
                        return Err(FrameError::SchemaCrcMismatch);
                    }
                }
            }
        }

        self.assembly = Some(asm);
        Ok(())
    }

    fn progress_event(&self) -> Option<SchemaEvent> {
        let asm = self.assembly?;
        if let Some(total) = asm.expected_total
            && asm.received == total
        {
            return Some(SchemaEvent::Complete {
                sid: asm.sid,
                schema_len: total,
            });
        }
        Some(SchemaEvent::Progress {
            sid: asm.sid,
            received: asm.received,
            expected_total: asm.expected_total,
        })
    }
}

/// CRC-32/IEEE parameters:
/// poly=0x04C11DB7, init=0xFFFFFFFF, refin=true, refout=true, xorout=0xFFFFFFFF
fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = CRCu32::crc32();
    crc.digest(data);
    crc.get_crc()
}

fn cbor_bstr_total_len(buf: &[u8]) -> Result<Option<usize>, FrameError> {
    // Intentionally decode only the CBOR bstr prefix (max 5 bytes) needed for total length.
    // This keeps schema-length discovery bounded and no-alloc in no_std targets.
    if buf.is_empty() {
        return Ok(None);
    }
    let first = buf[0];
    let major = first >> 5;
    if major != 2 {
        return Err(FrameError::InvalidCborLengthPrefix);
    }
    let ai = first & 0x1F;

    match ai {
        0..=23 => Ok(Some(1 + ai as usize)),
        24 => {
            if buf.len() < 2 {
                return Ok(None);
            }
            Ok(Some(2 + buf[1] as usize))
        }
        25 => {
            if buf.len() < 3 {
                return Ok(None);
            }
            let n = u16::from_be_bytes([buf[1], buf[2]]) as usize;
            Ok(Some(3 + n))
        }
        26 => {
            if buf.len() < 5 {
                return Ok(None);
            }
            let n = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            Ok(Some(5 + n))
        }
        _ => Err(FrameError::InvalidCborLengthPrefix),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vft(schema_start: bool, sid_mode: SidMode) -> u8 {
        Vft {
            version: PROTOCOL_VERSION,
            schema_start,
            sid_mode,
        }
        .encode()
        .unwrap()
    }

    #[test]
    fn crc_check_vector() {
        assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn parse_sid8_start_x8_s2() {
        let frame = [vft(true, SidMode::Sid8), 0x7A, 0x40, 1, 2, 3, 4, 5];
        let parsed = parse_frame(&frame, 2, PROTOCOL_VERSION).unwrap();
        assert_eq!(parsed.sid, Some(Sid::Sid8(0x7A)));
        assert_eq!(parsed.schema_chunk, &[0x40]);
        assert_eq!(parsed.payload, &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn parse_sid32_start_x8_s5() {
        let frame = [vft(true, SidMode::Sid32), 1, 2, 3, 4, 0x40, 0xAA, 0xBB];
        let parsed = parse_frame(&frame, 5, PROTOCOL_VERSION).unwrap();
        assert_eq!(parsed.sid, Some(Sid::Sid32(0x0403_0201)));
        assert_eq!(parsed.schema_chunk, &[0x40]);
        assert_eq!(parsed.payload, &[0xAA, 0xBB]);
    }

    #[test]
    fn receiver_installs_empty_bstr_sid8() {
        let schema = [0x40u8]; // CBOR empty bstr
        let sid8 = (crc32_ieee(&schema) & 0xFF) as u8;
        let mut r: Receiver<64> = Receiver::new(64, 8);

        let frame = [vft(true, SidMode::Sid8), sid8, 0x40, 1, 2, 3, 4, 5];
        let out = r.process_frame(&frame, 2).unwrap();
        assert_eq!(out.payload, &[1, 2, 3, 4, 5]);
        // Schema fits in one frame, so we get Complete directly.
        assert!(matches!(
            out.event,
            Some(SchemaEvent::Complete {
                sid: Sid::Sid8(_),
                schema_len: 1
            })
        ));

        let frame2 = [
            vft(false, SidMode::Sid8),
            SCHEMA_PAD_BYTE,
            SCHEMA_PAD_BYTE,
            9,
            9,
            9,
            9,
            9,
        ];
        let out2 = r.process_frame(&frame2, 2).unwrap();
        assert!(matches!(
            out2.event,
            Some(SchemaEvent::Complete {
                sid: Sid::Sid8(_),
                schema_len: 1
            })
        ));
    }

    #[test]
    fn receiver_sid32_two_frames() {
        // Schema stream is a bstr containing "{}": 0x41 0xA0
        let schema = [0x41u8, 0xA0];
        let sid = crc32_ieee(&schema);
        let sid_le = sid.to_le_bytes();
        let mut r: Receiver<64> = Receiver::new(64, 8);

        // X=8, S=5: start frame contributes 1 schema byte after sid prefix.
        let f1 = [
            vft(true, SidMode::Sid32),
            sid_le[0],
            sid_le[1],
            sid_le[2],
            sid_le[3],
            schema[0],
            0x11,
            0x22,
        ];
        let out1 = r.process_frame(&f1, 5).unwrap();
        assert_eq!(out1.payload, &[0x11, 0x22]);
        assert!(matches!(
            out1.event,
            Some(SchemaEvent::Started { sid: Sid::Sid32(_) })
        ));

        let f2 = [
            vft(false, SidMode::Sid32),
            schema[1],
            SCHEMA_PAD_BYTE,
            SCHEMA_PAD_BYTE,
            SCHEMA_PAD_BYTE,
            SCHEMA_PAD_BYTE,
            0x33,
            0x44,
        ];
        let out2 = r.process_frame(&f2, 5).unwrap();
        assert!(matches!(
            out2.event,
            Some(SchemaEvent::Complete {
                sid: Sid::Sid32(_),
                schema_len: 2
            })
        ));
        assert_eq!(out2.payload, &[0x33, 0x44]);
    }

    #[test]
    fn vft_parse_rejects_reserved_bits() {
        let bad = 0b0000_0001;
        let err = Vft::parse(bad).unwrap_err();
        assert_eq!(err, FrameError::ReservedBitsSet);
    }

    #[test]
    fn vft_encode_rejects_invalid_version() {
        let v = Vft {
            version: 4,
            schema_start: false,
            sid_mode: SidMode::Sid8,
        };
        let err = v.encode().unwrap_err();
        assert_eq!(err, FrameError::InvalidVersion);
    }

    #[test]
    fn parse_frame_rejects_too_short() {
        let frame = [vft(false, SidMode::Sid8)];
        let err = parse_frame(&frame, 2, PROTOCOL_VERSION).unwrap_err();
        assert_eq!(err, FrameError::FrameTooShort);
    }

    #[test]
    fn parse_frame_rejects_wrong_version() {
        let frame = [vft(false, SidMode::Sid8), 0x40, 1, 2];
        let err = parse_frame(&frame, 2, 1).unwrap_err();
        assert_eq!(err, FrameError::WrongProtocolVersion);
    }

    #[test]
    fn parse_frame_rejects_schema_lane_too_small_for_sid32() {
        let frame = [vft(true, SidMode::Sid32), 1, 2, 3, 4, 5, 6, 7];
        let err = parse_frame(&frame, 4, PROTOCOL_VERSION).unwrap_err();
        assert_eq!(err, FrameError::InvalidSchemaLaneWidth);
    }

    #[test]
    fn parse_frame_rejects_zero_payload() {
        let frame = [vft(false, SidMode::Sid8), 0x40, 0xA5];
        let err = parse_frame(&frame, 2, PROTOCOL_VERSION).unwrap_err();
        assert_eq!(err, FrameError::InvalidPayloadLength);
    }

    #[test]
    fn receiver_exposes_active_sid_and_schema_bytes() {
        let schema = [0x40u8];
        let sid8 = (crc32_ieee(&schema) & 0xFF) as u8;
        let mut r: Receiver<16> = Receiver::new(16, 8);

        let f1 = [vft(true, SidMode::Sid8), sid8, 0x40, 9, 9, 9, 9, 9];
        let _ = r.process_frame(&f1, 2).unwrap();
        assert_eq!(r.active_sid(), Some(Sid::Sid8(sid8)));
        assert_eq!(r.schema_bytes(), Some(&schema[..]));

        let f2 = [
            vft(false, SidMode::Sid8),
            SCHEMA_PAD_BYTE,
            SCHEMA_PAD_BYTE,
            1,
            1,
            1,
            1,
            1,
        ];
        let _ = r.process_frame(&f2, 2).unwrap();
        assert_eq!(r.schema_bytes(), Some(&schema[..]));
    }

    #[test]
    fn receiver_completed_schema_ignores_budget_on_padding_frames() {
        let schema = [0x40u8]; // Empty CBOR bstr: completes on start frame.
        let sid8 = (crc32_ieee(&schema) & 0xFF) as u8;
        let mut r: Receiver<16> = Receiver::new(16, 1);

        let start = [vft(true, SidMode::Sid8), sid8, 0x40, 1, 1, 1, 1, 1];
        let pad = [
            vft(false, SidMode::Sid8),
            SCHEMA_PAD_BYTE,
            SCHEMA_PAD_BYTE,
            2,
            2,
            2,
            2,
            2,
        ];

        let _ = r.process_frame(&start, 2).unwrap();
        assert_eq!(r.schema_bytes(), Some(&schema[..]));

        // Must not fail even though max_schema_frames is already reached.
        let _ = r.process_frame(&pad, 2).unwrap();
        let _ = r.process_frame(&pad, 2).unwrap();
        assert_eq!(r.schema_bytes(), Some(&schema[..]));
    }

    #[test]
    fn receiver_rejects_crc_mismatch_sid32() {
        let mut r: Receiver<16> = Receiver::new(16, 8);
        let bad_sid = 0x1122_3344u32.to_le_bytes();

        // Schema stream len 2 (0x41, 0xA0), so mismatch is detected on second frame.
        let f1 = [
            vft(true, SidMode::Sid32),
            bad_sid[0],
            bad_sid[1],
            bad_sid[2],
            bad_sid[3],
            0x41,
            0,
            0,
        ];
        let _ = r.process_frame(&f1, 5).unwrap();
        let f2 = [
            vft(false, SidMode::Sid32),
            0xA0,
            SCHEMA_PAD_BYTE,
            SCHEMA_PAD_BYTE,
            SCHEMA_PAD_BYTE,
            SCHEMA_PAD_BYTE,
            0,
            0,
        ];
        let err = r.process_frame(&f2, 5).unwrap_err();
        assert_eq!(err, FrameError::SchemaCrcMismatch);
    }

    #[test]
    fn receiver_enforces_frame_budget() {
        // Total len=3 bytes (0x43 xx xx), with S=2 sid8 mode:
        // start gives 1 byte, each non-start gives 2 bytes.
        let schema = [0x43u8, 0xAA, 0xBB];
        let sid8 = (crc32_ieee(&schema) & 0xFF) as u8;
        let mut r: Receiver<16> = Receiver::new(16, 1);

        let f1 = [vft(true, SidMode::Sid8), sid8, 0x43, 9, 9, 9, 9, 9];
        let _ = r.process_frame(&f1, 2).unwrap();

        let f2 = [vft(false, SidMode::Sid8), 0xAA, 0xBB, 1, 1, 1, 1, 1];
        let err = r.process_frame(&f2, 2).unwrap_err();
        assert_eq!(err, FrameError::SchemaFrameBudgetExceeded);
    }

    #[test]
    fn receiver_rejects_schema_too_large_after_len_discovery() {
        // bstr with ai=24, length=2 -> total stream len = 4 (0x58, 0x02, xx, xx)
        let mut r: Receiver<16> = Receiver::new(3, 8);
        let sid8 = 0u8;

        let f1 = [vft(true, SidMode::Sid8), sid8, 0x58, 9, 9, 9, 9, 9];
        let _ = r.process_frame(&f1, 2).unwrap();
        let f2 = [vft(false, SidMode::Sid8), 0x02, 0xAA, 1, 1, 1, 1, 1];
        let err = r.process_frame(&f2, 2).unwrap_err();
        assert_eq!(err, FrameError::SchemaTooLarge);
    }

    #[test]
    fn receiver_progress_event_is_emitted() {
        // Total schema stream len = 11 bytes (0x4A + 10 payload bytes).
        let schema = [0x4Au8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let sid = crc32_ieee(&schema).to_le_bytes();
        let mut r: Receiver<64> = Receiver::new(64, 8);

        let f1 = [
            vft(true, SidMode::Sid32),
            sid[0],
            sid[1],
            sid[2],
            sid[3],
            0x4A,
            0,
            0,
        ];
        let _ = r.process_frame(&f1, 5).unwrap();

        let f2 = [vft(false, SidMode::Sid32), 1, 2, 3, 4, 5, 0, 0];
        let out2 = r.process_frame(&f2, 5).unwrap();
        assert!(matches!(
            out2.event,
            Some(SchemaEvent::Progress {
                sid: Sid::Sid32(_),
                received: 6,
                expected_total: Some(11)
            })
        ));
    }

    #[test]
    fn cbor_bstr_len_parser_edges() {
        assert_eq!(cbor_bstr_total_len(&[]).unwrap(), None);
        assert_eq!(
            cbor_bstr_total_len(&[0xA0]).unwrap_err(),
            FrameError::InvalidCborLengthPrefix
        );

        // ai=24
        assert_eq!(cbor_bstr_total_len(&[0x58]).unwrap(), None);
        assert_eq!(cbor_bstr_total_len(&[0x58, 0x02]).unwrap(), Some(4));

        // ai=25
        assert_eq!(cbor_bstr_total_len(&[0x59, 0x00]).unwrap(), None);
        assert_eq!(cbor_bstr_total_len(&[0x59, 0x00, 0x03]).unwrap(), Some(6));

        // ai=26
        assert_eq!(cbor_bstr_total_len(&[0x5A, 0, 0, 0]).unwrap(), None);
        assert_eq!(
            cbor_bstr_total_len(&[0x5A, 0, 0, 0, 0x02]).unwrap(),
            Some(7)
        );
    }

    #[test]
    fn write_then_parse_sid8_start_roundtrip() {
        let mut frame = [0u8; 8];
        write_frame(FrameWrite {
            out_frame: &mut frame,
            schema_lane_bytes: 2,
            schema_start: true,
            sid_mode: SidMode::Sid8,
            sid: Some(Sid::Sid8(0x7A)),
            schema_chunk: &[0x40],
            payload: &[1, 2, 3, 4, 5],
        })
        .unwrap();

        let parsed = parse_frame(&frame, 2, PROTOCOL_VERSION).unwrap();
        assert_eq!(parsed.sid, Some(Sid::Sid8(0x7A)));
        assert_eq!(parsed.schema_chunk, &[0x40]);
        assert_eq!(parsed.payload, &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn write_then_parse_sid32_nonstart_roundtrip_with_padding() {
        let mut frame = [0u8; 8];
        write_frame(FrameWrite {
            out_frame: &mut frame,
            schema_lane_bytes: 5,
            schema_start: false,
            sid_mode: SidMode::Sid32,
            sid: None,
            schema_chunk: &[0x41, 0xA0],
            payload: &[0xAA, 0xBB],
        })
        .unwrap();

        let parsed = parse_frame(&frame, 5, PROTOCOL_VERSION).unwrap();
        assert_eq!(parsed.sid, None);
        assert_eq!(&parsed.schema_chunk[..2], &[0x41, 0xA0]);
        assert_eq!(&parsed.schema_chunk[2..], &[SCHEMA_PAD_BYTE; 3]);
        assert_eq!(parsed.payload, &[0xAA, 0xBB]);
    }

    #[test]
    fn write_frame_rejects_bad_sid_contracts() {
        let mut frame = [0u8; 8];
        let err = write_frame(FrameWrite {
            out_frame: &mut frame,
            schema_lane_bytes: 2,
            schema_start: true,
            sid_mode: SidMode::Sid8,
            sid: None,
            schema_chunk: &[0x40],
            payload: &[1, 2, 3, 4, 5],
        })
        .unwrap_err();
        assert_eq!(err, FrameError::MissingSidOnStart);

        let err2 = write_frame(FrameWrite {
            out_frame: &mut frame,
            schema_lane_bytes: 2,
            schema_start: false,
            sid_mode: SidMode::Sid8,
            sid: Some(Sid::Sid8(1)),
            schema_chunk: &[0x40, 0xA5],
            payload: &[1, 2, 3, 4, 5],
        })
        .unwrap_err();
        assert_eq!(err2, FrameError::UnexpectedSidOnNonStart);
    }

    #[cfg(feature = "diagnostic-errors")]
    #[test]
    fn diagnostic_parse_reports_expected_actual() {
        let frame = [0x30, 1, 2, 3, 4, 0x40, 0xAA, 0xBB];
        let err = parse_frame_detailed(&frame, 2, PROTOCOL_VERSION).unwrap_err();
        assert_eq!(err.kind, FrameError::InvalidSchemaLaneWidth);
        assert_eq!(err.expected, Some(5));
        assert_eq!(err.actual, Some(2));
    }
}
