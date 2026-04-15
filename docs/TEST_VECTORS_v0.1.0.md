# Telemutter Envelope Test Vectors v0.1.0

These vectors are normative for envelope v0.1.0.
All hex bytes are space-separated.

## Vector A: sid8 start, minimal X=8/S=2

- Purpose: parse/write baseline
- `X=8`, `S=2`
- `VFT=0x20` (`version=0`, `schema_start=1`, `sid_mode=sid8`)
- SID: `0x7A`
- Schema lane payload byte: `0x40`
- Payload: `01 02 03 04 05`

Frame:

`20 7A 40 01 02 03 04 05`

## Vector B: sid32 start, X=16/S=5

- Purpose: parse/write baseline
- `X=16`, `S=5`
- `VFT=0x30` (`schema_start=1`, `sid_mode=sid32`)
- SID32: `0x12345678` (little-endian on wire: `78 56 34 12`)
- Schema lane payload byte: `40`
- Payload: `A0 A1 A2 A3 A4 A5 A6 A7 A8 A9`

Frame:

`30 78 56 34 12 40 A0 A1 A2 A3 A4 A5 A6 A7 A8 A9`

## Vector C: sid32 non-start with explicit padding in schema lane

- Purpose: lane padding alignment check
- `X=16`, `S=5`
- `VFT=0x10` (`schema_start=0`, `sid_mode=sid32`)
- Schema chunk bytes: `C1 C2`
- Required pad bytes: `A5 A5 A5`
- Payload: `B0 B1 B2 B3 B4 B5 B6 B7 B8 B9`

Frame:

`10 C1 C2 A5 A5 A5 B0 B1 B2 B3 B4 B5 B6 B7 B8 B9`

## Vector D: receiver-valid sid8 complete schema

- Purpose: schema install path
- Schema bytes: `40`
- CRC32/IEEE(`40`) = `A4DEAE1D`
- sid8 is low byte: `1D`
- `X=8`, `S=2`

Frame:

`20 1D 40 01 02 03 04 05`

Expected:

- Receiver installs schema `40`

## Vector E: receiver-valid sid32 two-frame schema

- Purpose: schema reassembly + CRC check
- Schema bytes: `43 01 02 03`
- CRC32/IEEE(`43 01 02 03`) = `021871C0`
- sid32 on wire little-endian: `C0 71 18 02`
- `X=16`, `S=5`

Frame 0 (start):

`30 C0 71 18 02 43 10 11 12 13 14 15 16 17 18 19`

Frame 1 (continuation, pad after completion):

`10 01 02 03 A5 A5 20 21 22 23 24 25 26 27 28 29`

Expected:

- Receiver installs schema `43 01 02 03`

## Vector F: CBOR length edge (23-byte bstr)

- Purpose: first-byte length edge
- First schema byte should be `57` (major=2, ai=23)

Example start frame (`X=8`, `S=2`, sid8 arbitrary `AA`):

`20 AA 57 00 00 00 00 00`

## Vector G: CBOR length edge (24-byte bstr)

- Purpose: first-byte transition edge (`ai=24`)
- First schema prefix byte should be `58`

Example start frame (`X=8`, `S=2`, sid8 arbitrary `BB`):

`20 BB 58 00 00 00 00 00`

## Negative Vectors (MUST reject)

### Vector X: Reserved bits set in VFT

- Purpose: header sanity rejection
- `X=8`, `S=2`
- `VFT=0x21` (`b0` reserved set)

Frame:

`21 7A 40 01 02 03 04 05`

Expected:

- Reject with reserved-bits error

### Vector Y: Wrong protocol version

- Purpose: version gate
- `X=8`, `S=2`
- `VFT=0x60` (`version=1`, start+sid8 style)

Frame:

`60 7A 40 01 02 03 04 05`

Expected:

- Reject with wrong-version error (for expected version 0)

### Vector Z: Invalid schema lane width for sid32

- Purpose: `S` vs `sid_mode` minimum enforcement
- `X=8`, `S=2` (invalid for sid32)
- `VFT=0x30` (`schema_start=1`, `sid_mode=sid32`)

Frame:

`30 01 02 03 04 40 AA BB`

Expected:

- Reject with invalid-schema-lane-width error
