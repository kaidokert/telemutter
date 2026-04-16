import unittest

from telemutter_py.envelope import (
    FrameWrite,
    Receiver,
    Sid,
    SidMode,
    parse_frame,
    write_frame,
)


class GoldenVectorTests(unittest.TestCase):
    def test_vector_a_parse_sid8_start_minimal(self) -> None:
        frame = bytes([0x20, 0x7A, 0x40, 0x01, 0x02, 0x03, 0x04, 0x05])
        p = parse_frame(frame, 2, 0)
        self.assertEqual(p.sid, Sid(SidMode.SID8, 0x7A))
        self.assertEqual(p.schema_chunk, b"\x40")
        self.assertEqual(p.payload, b"\x01\x02\x03\x04\x05")

    def test_vector_b_parse_sid32_start(self) -> None:
        frame = bytes(
            [
                0x30,
                0x78,
                0x56,
                0x34,
                0x12,
                0x40,
                0xA0,
                0xA1,
                0xA2,
                0xA3,
                0xA4,
                0xA5,
                0xA6,
                0xA7,
                0xA8,
                0xA9,
            ]
        )
        p = parse_frame(frame, 5, 0)
        self.assertEqual(p.sid, Sid(SidMode.SID32, 0x12345678))
        self.assertEqual(p.schema_chunk, b"\x40")
        self.assertEqual(p.payload, bytes([0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9]))

    def test_vector_d_receiver_valid_sid8_installs(self) -> None:
        frame = bytes([0x20, 0x1D, 0x40, 0x01, 0x02, 0x03, 0x04, 0x05])
        r = Receiver(max_schema_bytes=16, max_schema_frames=8)
        r.process_frame(frame, 2)
        self.assertEqual(r.schema_bytes(), b"\x40")

    def test_vector_e_receiver_valid_sid32_two_frame_installs(self) -> None:
        f0 = bytes([0x30, 0xC0, 0x71, 0x18, 0x02, 0x43, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19])
        f1 = bytes([0x10, 0x01, 0x02, 0x03, 0xA5, 0xA5, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29])
        r = Receiver(max_schema_bytes=32, max_schema_frames=8)
        r.process_frame(f0, 5)
        r.process_frame(f1, 5)
        self.assertEqual(r.schema_bytes(), bytes([0x43, 0x01, 0x02, 0x03]))

    def test_vector_a_write_matches_exact_hex(self) -> None:
        out = write_frame(
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
        self.assertEqual(out, bytes([0x20, 0x7A, 0x40, 0x01, 0x02, 0x03, 0x04, 0x05]))


if __name__ == "__main__":
    unittest.main()
