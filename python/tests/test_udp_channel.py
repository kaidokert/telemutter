import socket
import unittest

from telemutter_py.envelope import FrameError, FrameWrite, Receiver, Sid, SidMode, crc32_ieee, write_frame


def schema_bstr(body: bytes) -> bytes:
    n = len(body)
    if n <= 23:
        return bytes([0x40 | n]) + body
    return bytes([0x58, n]) + body


class UdpChannelTests(unittest.TestCase):
    def test_udp_loopback_converges(self) -> None:
        x = 16
        s = 2
        schema = schema_bstr(b"udp-schema")
        sid = crc32_ieee(schema) & 0xFF

        rx = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        tx = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        rx.bind(("127.0.0.1", 0))
        rx.settimeout(0.2)
        addr = rx.getsockname()

        def mk(schema_start: bool, sid_obj: Sid | None, chunk: bytes, fill: int) -> bytes:
            return write_frame(
                FrameWrite(
                    frame_len=x,
                    schema_lane_bytes=s,
                    schema_start=schema_start,
                    sid_mode=SidMode.SID8,
                    sid=sid_obj,
                    schema_chunk=chunk,
                    payload=bytes([fill] * (x - 1 - s)),
                )
            )

        frames = [mk(True, Sid(SidMode.SID8, sid), schema[:1], 0x11)]
        i = 1
        while i < len(schema):
            frames.append(mk(False, None, schema[i : i + s], 0x22))
            i += s

        for _ in range(3):
            for f in frames:
                tx.sendto(f, addr)

        r = Receiver(max_schema_bytes=256, max_schema_frames=256)
        ok = False
        try:
            for _ in range(256):
                data, _peer = rx.recvfrom(128)
                if len(data) != x:
                    continue
                try:
                    r.process_frame(data, s)
                except FrameError:
                    continue
                if r.schema_bytes() == schema:
                    ok = True
                    break
        finally:
            rx.close()
            tx.close()

        self.assertTrue(ok)


if __name__ == "__main__":
    unittest.main()
