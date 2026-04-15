#!/usr/bin/env python3
from __future__ import annotations

import argparse
import socket
import sys
import time

from telemutter_py.envelope import FrameError, FrameWrite, Receiver, Sid, SidMode, crc32_ieee, write_frame

X = 16
S = 2
SCHEMA_BODY = b"interop-v1"
SCHEMA = bytes([0x40 | len(SCHEMA_BODY)]) + SCHEMA_BODY
SID8 = crc32_ieee(SCHEMA) & 0xFF


def epoch_frames() -> list[bytes]:
    payload_len = X - 1 - S
    out: list[bytes] = []
    out.append(
        write_frame(
            FrameWrite(
                frame_len=X,
                schema_lane_bytes=S,
                schema_start=True,
                sid_mode=SidMode.SID8,
                sid=Sid(SidMode.SID8, SID8),
                schema_chunk=SCHEMA[:1],
                payload=bytes([0x11] * payload_len),
            )
        )
    )
    i = 1
    while i < len(SCHEMA):
        out.append(
            write_frame(
                FrameWrite(
                    frame_len=X,
                    schema_lane_bytes=S,
                    schema_start=False,
                    sid_mode=SidMode.SID8,
                    sid=None,
                    schema_chunk=SCHEMA[i : i + S],
                    payload=bytes([0x22] * payload_len),
                )
            )
        )
        i += S
    return out


def mode_send(host: str, port: int, loops: int) -> int:
    tx = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        frames = epoch_frames()
        for _ in range(loops):
            for f in frames:
                tx.sendto(f, (host, port))
    finally:
        tx.close()
    return 0


def mode_recv(host: str, port: int, timeout_ms: int) -> int:
    rx = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    rx.bind((host, port))
    # Handshake for parent process: confirms bind success and chosen port when --port=0.
    actual_port = rx.getsockname()[1]
    print(f"PORT:{actual_port}", flush=True)
    rx.settimeout(timeout_ms / 1000.0)
    r = Receiver(max_schema_bytes=256, max_schema_frames=512)
    deadline = time.time() + (timeout_ms / 1000.0)
    try:
        while time.time() < deadline:
            try:
                data, _ = rx.recvfrom(256)
            except socket.timeout:
                continue
            if len(data) != X:
                continue
            try:
                r.process_frame(data, S)
            except FrameError:
                continue
            if r.schema_bytes() == SCHEMA:
                return 0
        return 2
    finally:
        rx.close()


def main() -> int:
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="mode", required=True)

    s = sub.add_parser("send")
    s.add_argument("--host", default="127.0.0.1")
    s.add_argument("--port", type=int, required=True)
    s.add_argument("--loops", type=int, default=4)

    r = sub.add_parser("recv")
    r.add_argument("--host", default="127.0.0.1")
    r.add_argument("--port", type=int, required=True)
    r.add_argument("--timeout-ms", type=int, default=2000)

    args = p.parse_args()
    if args.mode == "send":
        return mode_send(args.host, args.port, args.loops)
    return mode_recv(args.host, args.port, args.timeout_ms)


if __name__ == "__main__":
    sys.exit(main())
