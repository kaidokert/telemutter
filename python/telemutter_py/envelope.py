from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
import binascii


PROTOCOL_VERSION = 0
SCHEMA_PAD_BYTE = 0xA5


class FrameError(Exception):
    def __init__(self, code: str, expected: int | None = None, actual: int | None = None) -> None:
        self.code = code
        self.expected = expected
        self.actual = actual
        super().__init__(self.__str__())

    def __str__(self) -> str:
        parts = [self.code]
        if self.expected is not None:
            parts.append(f"expected={self.expected}")
        if self.actual is not None:
            parts.append(f"actual={self.actual}")
        return " ".join(parts)


class SidMode(Enum):
    SID8 = 0
    SID32 = 1


@dataclass(frozen=True)
class Sid:
    mode: SidMode
    value: int


@dataclass(frozen=True)
class Vft:
    version: int
    schema_start: bool
    sid_mode: SidMode

    @staticmethod
    def parse(byte: int) -> "Vft":
        version = (byte >> 6) & 0b11
        schema_start = ((byte >> 5) & 1) != 0
        sid_mode = SidMode.SID32 if ((byte >> 4) & 1) else SidMode.SID8
        reserved = byte & 0x0F
        if reserved != 0:
            raise FrameError("ReservedBitsSet")
        return Vft(version=version, schema_start=schema_start, sid_mode=sid_mode)

    def encode(self) -> int:
        if not (0 <= self.version <= 0b11):
            raise FrameError("InvalidVersion")
        out = (self.version & 0b11) << 6
        if self.schema_start:
            out |= 1 << 5
        if self.sid_mode == SidMode.SID32:
            out |= 1 << 4
        return out


@dataclass(frozen=True)
class ParsedFrame:
    vft: Vft
    sid: Sid | None
    schema_chunk: bytes
    payload: bytes


@dataclass(frozen=True)
class FrameWrite:
    frame_len: int
    schema_lane_bytes: int
    schema_start: bool
    sid_mode: SidMode
    sid: Sid | None
    schema_chunk: bytes
    payload: bytes


@dataclass(frozen=True)
class SchemaStarted:
    sid: Sid


@dataclass(frozen=True)
class SchemaProgress:
    sid: Sid
    received: int
    expected_total: int | None


@dataclass(frozen=True)
class SchemaComplete:
    sid: Sid
    schema_len: int


@dataclass(frozen=True)
class ProcessedFrame:
    payload: bytes
    event: SchemaStarted | SchemaProgress | SchemaComplete | None


@dataclass
class AssemblyState:
    sid: Sid
    received: int
    expected_total: int | None
    frames_seen: int


def crc32_ieee(data: bytes | bytearray | memoryview) -> int:
    return binascii.crc32(data) & 0xFFFFFFFF


def _sid_prefix_len(sid_mode: SidMode) -> int:
    return 1 if sid_mode == SidMode.SID8 else 4


def parse_frame(frame: bytes, schema_lane_bytes: int, expected_version: int = PROTOCOL_VERSION) -> ParsedFrame:
    if len(frame) < 2:
        raise FrameError("FrameTooShort", expected=2, actual=len(frame))

    vft = Vft.parse(frame[0])
    if vft.version != expected_version:
        raise FrameError("WrongProtocolVersion", expected=expected_version, actual=vft.version)

    sid_prefix_len = _sid_prefix_len(vft.sid_mode)
    if schema_lane_bytes < sid_prefix_len + 1:
        raise FrameError("InvalidSchemaLaneWidth", expected=sid_prefix_len + 1, actual=schema_lane_bytes)

    payload_len = len(frame) - 1 - schema_lane_bytes
    if payload_len <= 0:
        raise FrameError("InvalidPayloadLength", expected=1, actual=max(payload_len, 0))

    lane = frame[1 : 1 + schema_lane_bytes]
    payload = frame[1 + schema_lane_bytes :]
    sid: Sid | None = None
    if vft.schema_start:
        if vft.sid_mode == SidMode.SID8:
            sid = Sid(SidMode.SID8, lane[0])
            schema_chunk = lane[1:]
        else:
            sid = Sid(SidMode.SID32, int.from_bytes(lane[:4], byteorder="little", signed=False))
            schema_chunk = lane[4:]
    else:
        schema_chunk = lane
    return ParsedFrame(vft=vft, sid=sid, schema_chunk=schema_chunk, payload=payload)


def write_frame(args: FrameWrite) -> bytes:
    sid_prefix_len = _sid_prefix_len(args.sid_mode)
    if args.schema_lane_bytes < sid_prefix_len + 1:
        raise FrameError("InvalidSchemaLaneWidth", expected=sid_prefix_len + 1, actual=args.schema_lane_bytes)
    if args.frame_len < 2:
        raise FrameError("FrameTooShort", expected=2, actual=args.frame_len)

    payload_len = args.frame_len - 1 - args.schema_lane_bytes
    if payload_len <= 0:
        raise FrameError("InvalidPayloadLength", expected=1, actual=max(payload_len, 0))
    if len(args.payload) != payload_len:
        raise FrameError("PayloadLenMismatch", expected=payload_len, actual=len(args.payload))

    if args.schema_start and args.sid is None:
        raise FrameError("MissingSidOnStart")
    if (not args.schema_start) and args.sid is not None:
        raise FrameError("UnexpectedSidOnNonStart")

    if args.sid is not None and args.sid.mode != args.sid_mode:
        raise FrameError("SidModeMismatch")

    schema_cap = args.schema_lane_bytes - sid_prefix_len if args.schema_start else args.schema_lane_bytes
    if len(args.schema_chunk) > schema_cap:
        raise FrameError("SchemaChunkTooLong", expected=schema_cap, actual=len(args.schema_chunk))

    out = bytearray(args.frame_len)
    out[0] = Vft(version=PROTOCOL_VERSION, schema_start=args.schema_start, sid_mode=args.sid_mode).encode()
    lane_start = 1
    lane_end = lane_start + args.schema_lane_bytes
    out[lane_start:lane_end] = bytes([SCHEMA_PAD_BYTE]) * args.schema_lane_bytes

    cursor = lane_start
    if args.schema_start:
        assert args.sid is not None
        if args.sid_mode == SidMode.SID8:
            out[cursor] = args.sid.value & 0xFF
            cursor += 1
        else:
            out[cursor : cursor + 4] = (args.sid.value & 0xFFFFFFFF).to_bytes(4, "little")
            cursor += 4

    out[cursor : cursor + len(args.schema_chunk)] = args.schema_chunk
    out[lane_end:] = args.payload
    return bytes(out)


def _cbor_bstr_total_len_from_buf(buf: bytearray, received: int) -> int | None:
    # Parse only CBOR bstr prefix bytes (max 5) needed for total length discovery.
    if received == 0:
        return None
    first = buf[0]
    major = (first >> 5) & 0x07
    ai = first & 0x1F
    if major != 2:
        raise FrameError("InvalidCborLengthPrefix")
    if ai <= 23:
        return 1 + ai
    if ai == 24:
        if received < 2:
            return None
        return 2 + buf[1]
    if ai == 25:
        if received < 3:
            return None
        return 3 + ((buf[1] << 8) | buf[2])
    if ai == 26:
        if received < 5:
            return None
        return 5 + ((buf[1] << 24) | (buf[2] << 16) | (buf[3] << 8) | buf[4])
    raise FrameError("InvalidCborLengthPrefix")


class Receiver:
    def __init__(self, max_schema_bytes: int, max_schema_frames: int, buf_capacity: int | None = None) -> None:
        self.max_schema_bytes = max_schema_bytes
        self.max_schema_frames = max_schema_frames
        self.buf_capacity = max_schema_bytes if buf_capacity is None else buf_capacity
        self.schema_buf = bytearray(self.buf_capacity)
        self.assembly: AssemblyState | None = None
        self._active_sid: Sid | None = None

    def active_sid(self) -> Sid | None:
        return self._active_sid

    def schema_bytes(self) -> bytes | None:
        if self.assembly is None:
            return None
        if self.assembly.expected_total == self.assembly.received:
            n = self.assembly.received
            return bytes(self.schema_buf[:n])
        return None

    def _abort_assembly(self) -> None:
        self.assembly = None
        self._active_sid = None

    def process_frame(self, frame: bytes, schema_lane_bytes: int) -> ProcessedFrame:
        parsed = parse_frame(frame, schema_lane_bytes, PROTOCOL_VERSION)
        if parsed.vft.schema_start:
            if parsed.sid is None:
                raise FrameError("MissingSidOnStart")
            self.assembly = AssemblyState(parsed.sid, 0, None, 0)
            self._active_sid = parsed.sid
            self._append_schema_chunk(parsed.schema_chunk)
            evt = self._progress_event()
            if isinstance(evt, SchemaComplete):
                event: SchemaStarted | SchemaProgress | SchemaComplete | None = evt
            else:
                event = SchemaStarted(parsed.sid)
            return ProcessedFrame(payload=parsed.payload, event=event)

        self._append_schema_chunk(parsed.schema_chunk)
        return ProcessedFrame(payload=parsed.payload, event=self._progress_event())

    def _append_schema_chunk(self, chunk: bytes) -> None:
        if self.assembly is None:
            return
        asm = self.assembly

        # Once complete, ignore subsequent schema-lane bytes (typically padding) with zero work.
        if asm.expected_total is not None and asm.received >= asm.expected_total:
            return

        asm.frames_seen += 1
        if asm.frames_seen > self.max_schema_frames:
            self._abort_assembly()
            raise FrameError("SchemaFrameBudgetExceeded")
        chunk_off = 0
        chunk_len = len(chunk)

        # Prefix-discovery phase: copy only until bstr total length is known.
        if asm.expected_total is None:
            while chunk_off < chunk_len and asm.expected_total is None:
                if asm.received >= self.max_schema_bytes or asm.received >= self.buf_capacity:
                    self._abort_assembly()
                    raise FrameError("SchemaTooLarge", expected=self.max_schema_bytes, actual=asm.received + 1)

                self.schema_buf[asm.received] = chunk[chunk_off]
                asm.received += 1
                chunk_off += 1

                try:
                    maybe_total = _cbor_bstr_total_len_from_buf(self.schema_buf, asm.received)
                except FrameError:
                    self._abort_assembly()
                    raise
                asm.expected_total = maybe_total
                if maybe_total is not None and (maybe_total > self.max_schema_bytes or maybe_total > self.buf_capacity):
                    self._abort_assembly()
                    raise FrameError("SchemaTooLarge", expected=self.max_schema_bytes, actual=maybe_total)

        # Bulk-copy phase once we know how much schema stream remains.
        if asm.expected_total is not None and chunk_off < chunk_len and asm.received < asm.expected_total:
            remaining = asm.expected_total - asm.received
            take = min(chunk_len - chunk_off, remaining)
            end = asm.received + take
            self.schema_buf[asm.received:end] = chunk[chunk_off : chunk_off + take]
            asm.received = end

        if asm.expected_total is not None and asm.received == asm.expected_total:
            schema = memoryview(self.schema_buf)[: asm.expected_total]
            sid: Sid = asm.sid
            if sid.mode == SidMode.SID32:
                if crc32_ieee(schema) != (sid.value & 0xFFFFFFFF):
                    self._abort_assembly()
                    raise FrameError("SchemaCrcMismatch")
            else:
                if (crc32_ieee(schema) & 0xFF) != (sid.value & 0xFF):
                    self._abort_assembly()
                    raise FrameError("SchemaCrcMismatch")

        self.assembly = asm

    def _progress_event(self) -> SchemaProgress | SchemaComplete | None:
        if self.assembly is None:
            return None
        sid: Sid = self.assembly.sid
        received: int = self.assembly.received
        total: int | None = self.assembly.expected_total
        if total is not None and received == total:
            return SchemaComplete(sid=sid, schema_len=total)
        return SchemaProgress(sid=sid, received=received, expected_total=total)
