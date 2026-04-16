import unittest

from telemutter_py.envelope import FrameError, parse_frame


class NegativeGoldenVectorTests(unittest.TestCase):
    def test_vector_x_reserved_bits_set_rejected(self) -> None:
        frame = bytes([0x21, 0x7A, 0x40, 0x01, 0x02, 0x03, 0x04, 0x05])
        with self.assertRaisesRegex(FrameError, "ReservedBitsSet"):
            parse_frame(frame, 2, 0)

    def test_vector_y_wrong_version_rejected(self) -> None:
        frame = bytes([0x60, 0x7A, 0x40, 0x01, 0x02, 0x03, 0x04, 0x05])
        with self.assertRaisesRegex(FrameError, "WrongProtocolVersion"):
            parse_frame(frame, 2, 0)

    def test_vector_z_invalid_schema_lane_width_for_sid32_rejected(self) -> None:
        frame = bytes([0x30, 0x01, 0x02, 0x03, 0x04, 0x40, 0xAA, 0xBB])
        with self.assertRaisesRegex(FrameError, "InvalidSchemaLaneWidth"):
            parse_frame(frame, 2, 0)


if __name__ == "__main__":
    unittest.main()
