import unittest

from telemutter_py.envelope import (
    FrameError,
    FrameWrite,
    Receiver,
    Sid,
    SidMode,
    parse_frame,
    write_frame,
    crc32_ieee,
)


def schema_bstr(body: bytes) -> bytes:
    n = len(body)
    if n <= 23:
        return bytes([0x40 | n]) + body
    return bytes([0x58, n]) + body


class EnvelopeTests(unittest.TestCase):
    def test_write_parse_sid8_roundtrip(self) -> None:
        frame = write_frame(
            FrameWrite(
                frame_len=8,
                schema_lane_bytes=2,
                schema_start=True,
                sid_mode=SidMode.SID8,
                sid=Sid(SidMode.SID8, 0x7A),
                schema_chunk=b"\x40",
                payload=b"\x01\x02\x03\x04\x05",
            )
        )
        parsed = parse_frame(frame, 2)
        self.assertEqual(parsed.sid, Sid(SidMode.SID8, 0x7A))
        self.assertEqual(parsed.schema_chunk, b"\x40")
        self.assertEqual(parsed.payload, b"\x01\x02\x03\x04\x05")

    def test_receiver_installs_schema_sid8(self) -> None:
        schema = schema_bstr(b"abc")
        sid8 = crc32_ieee(schema) & 0xFF
        r = Receiver(max_schema_bytes=64, max_schema_frames=64)

        f0 = write_frame(
            FrameWrite(
                frame_len=8,
                schema_lane_bytes=2,
                schema_start=True,
                sid_mode=SidMode.SID8,
                sid=Sid(SidMode.SID8, sid8),
                schema_chunk=schema[:1],
                payload=b"\x11\x11\x11\x11\x11",
            )
        )
        f1 = write_frame(
            FrameWrite(
                frame_len=8,
                schema_lane_bytes=2,
                schema_start=False,
                sid_mode=SidMode.SID8,
                sid=None,
                schema_chunk=schema[1:3],
                payload=b"\x22\x22\x22\x22\x22",
            )
        )
        f2 = write_frame(
            FrameWrite(
                frame_len=8,
                schema_lane_bytes=2,
                schema_start=False,
                sid_mode=SidMode.SID8,
                sid=None,
                schema_chunk=schema[3:],
                payload=b"\x33\x33\x33\x33\x33",
            )
        )

        r.process_frame(f0, 2)
        r.process_frame(f1, 2)
        r.process_frame(f2, 2)
        self.assertEqual(r.schema_bytes(), schema)

    def test_schema_crc_mismatch_rejected(self) -> None:
        schema = schema_bstr(b"bad")
        wrong_sid = (crc32_ieee(schema) ^ 0x55) & 0xFF
        r = Receiver(max_schema_bytes=64, max_schema_frames=64)
        f0 = write_frame(
            FrameWrite(
                frame_len=8,
                schema_lane_bytes=2,
                schema_start=True,
                sid_mode=SidMode.SID8,
                sid=Sid(SidMode.SID8, wrong_sid),
                schema_chunk=schema[:1],
                payload=b"\x00\x00\x00\x00\x00",
            )
        )
        f1 = write_frame(
            FrameWrite(
                frame_len=8,
                schema_lane_bytes=2,
                schema_start=False,
                sid_mode=SidMode.SID8,
                sid=None,
                schema_chunk=schema[1:3],
                payload=b"\x00\x00\x00\x00\x00",
            )
        )
        f2 = write_frame(
            FrameWrite(
                frame_len=8,
                schema_lane_bytes=2,
                schema_start=False,
                sid_mode=SidMode.SID8,
                sid=None,
                schema_chunk=schema[3:],
                payload=b"\x00\x00\x00\x00\x00",
            )
        )
        r.process_frame(f0, 2)
        r.process_frame(f1, 2)
        with self.assertRaises(FrameError):
            r.process_frame(f2, 2)

    def test_completed_schema_ignores_budget_on_padding_frames(self) -> None:
        schema = b"\x40"  # Empty CBOR bstr, completes on start frame.
        sid8 = crc32_ieee(schema) & 0xFF
        r = Receiver(max_schema_bytes=16, max_schema_frames=1)

        start = write_frame(
            FrameWrite(
                frame_len=8,
                schema_lane_bytes=2,
                schema_start=True,
                sid_mode=SidMode.SID8,
                sid=Sid(SidMode.SID8, sid8),
                schema_chunk=schema,
                payload=b"\x01\x01\x01\x01\x01",
            )
        )
        pad = write_frame(
            FrameWrite(
                frame_len=8,
                schema_lane_bytes=2,
                schema_start=False,
                sid_mode=SidMode.SID8,
                sid=None,
                schema_chunk=b"",
                payload=b"\x02\x02\x02\x02\x02",
            )
        )

        r.process_frame(start, 2)
        self.assertEqual(r.schema_bytes(), schema)
        # Must not fail even though max_schema_frames is already reached.
        r.process_frame(pad, 2)
        r.process_frame(pad, 2)
        self.assertEqual(r.schema_bytes(), schema)


if __name__ == "__main__":
    unittest.main()
