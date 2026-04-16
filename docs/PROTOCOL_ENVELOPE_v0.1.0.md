# Telemutter Envelope Protocol v0.1.0 (Frozen)

Status: Frozen normative transport envelope for this repository.

Scope: This document specifies the fixed-frame envelope layer and its schema-stream framing contract. Payload semantics remain schema-defined and out of scope.

Directionality: This protocol is explicitly unidirectional (one-way). It defines no acknowledgments, retransmission signaling, receiver-to-sender diagnostics, or any other reverse control channel.

## 1. Frame Model

- Every frame has a fixed deployment size `X` bytes.
- Envelope layout:
  1. `VFT` (1 byte)
  2. Schema lane (`S` bytes, fixed per epoch/deployment context)
  3. Payload (`X - 1 - S` bytes)
- Payload MUST always be present, therefore `X - 1 - S >= 1`.

## 2. VFT Byte

Bit layout (`b7` MSB to `b0` LSB):

- `b7..b6`: `version` (2 bits)
- `b5`: `schema_start` (1 bit)
- `b4`: `sid_mode` (1 bit)
  - `0` = `sid8`
  - `1` = `sid32`
- `b3..b0`: reserved, MUST be zero

Normative rules:

- Implementations MUST reject frames where reserved bits are non-zero.
- `version` MUST match the expected protocol version.
- Current version is `0` (`PROTOCOL_VERSION = 0`).

## 3. Schema Lane Semantics

- Schema lane width `S` is externally configured and fixed for the active epoch.
- `sid_mode` defines required minimum lane capacity:
  - `sid8`: `S >= 2` (1 SID byte + at least 1 schema byte)
  - `sid32`: `S >= 5` (4 SID bytes + at least 1 schema byte)

### 3.1 Start Frame (`schema_start = 1`)

- Schema lane begins with explicit SID:
  - `sid8`: 1 byte
  - `sid32`: 4 bytes, little-endian
- Remaining bytes in schema lane are schema chunk bytes.

### 3.2 Non-Start Frame (`schema_start = 0`)

- No SID prefix appears in the lane.
- Entire lane is schema chunk bytes (or padding once complete).

## 4. Writing Rules

- Start frame MUST include SID matching `sid_mode`.
- Non-start frame MUST NOT include SID.
- Schema chunk bytes MUST fit the lane capacity of the frame type.
- Unused schema lane bytes MUST be filled with padding byte `0xA5`.

## 5. Schema Stream Format

- v0.1.0 NORMATIVE: schema stream is encoded as CBOR definite-length `bstr`.
- This is an intentional envelope-layer coupling in v0.1.0 (not payload parsing).
- Receiver derives total schema length from the CBOR `bstr` header:
  - AI `0..23`: 1-byte header
  - AI `24`: 2-byte header
  - AI `25`: 3-byte header
  - AI `26`: 5-byte header
- Indefinite lengths and invalid prefix forms are rejected at envelope assembly time.

## 6. Receiver State Machine

- On `schema_start = 1`:
  - Abort any current assembly.
  - Start new assembly for provided SID.
  - Set active SID to new SID.
- On non-start frame:
  - Append lane bytes to active assembly if one exists.
- Assembly stops at discovered total `bstr` length.

Limits:

- Receiver MUST enforce bounded limits:
  - `max_schema_bytes`
  - `max_schema_frames`
- Exceeding limits MUST abort current assembly.

Validation on completion:

- `sid32`: full schema CRC-32/IEEE MUST equal SID.
- `sid8`: low 8 bits of schema CRC-32/IEEE MUST equal SID.
- CRC mismatch MUST reject schema and abort assembly.

## 7. CRC Definition

CRC-32/IEEE parameters:

- Poly: `0x04C11DB7`
- Init: `0xFFFFFFFF`
- RefIn: `true`
- RefOut: `true`
- XorOut: `0xFFFFFFFF`

Check value:

- CRC("123456789") = `0xCBF43926`

## 8. Transport/Channel Notes

- Envelope parser is transport-agnostic.
- Channel adapters SHOULD pass only fixed-size frames (`X`) into envelope parsing.
- Oversized/undersized channel datagrams SHOULD be dropped before envelope parse.
- Any feedback or diagnostics transport is out of scope and, if needed, MUST be provided by a separate protocol/channel.

## 9. Schema Lane Budget Policy

- `S` is a fixed, preconfigured schema-lane width for an epoch.
- `S` MUST NOT vary frame-to-frame within an epoch.
- After schema convergence, schema lane bytes remain present and MUST be padded with `0xA5`.
- This is a deliberate constant-bandwidth tax of `S/X` per frame.
- Deployments SHOULD choose the smallest `S` that still meets required convergence time.

## 10. Compliance Target in This Repository

Reference implementation and tests:

- Core envelope: `telemutter/src/lib.rs`
- Adversarial tests: `telemutter/tests/adversarial_wire.rs`
- UDP integration tests: `telemutter/tests/udp_channel.rs`
- AVR entrypoint crate: `telemutter-avr`
- Cortex-M entrypoint crate: `telemutter-cortexm`

Toolchain policy:

- Rust Edition 2024 is intentional and required for this workspace.
- This project targets current Rust toolchains by design.
