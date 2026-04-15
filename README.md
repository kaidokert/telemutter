# telemutter

A `no_std` fixed-frame telemetry envelope for constrained targets (AVR, Cortex-M) with
a matching pure-Python stdlib implementation.

Telemutter multiplexes a **schema stream** alongside payload data in every frame,
so receivers can self-discover the wire format without out-of-band configuration.
Schema identity is CRC-verified (SID8 or SID32) and assembly is bounded by
configurable byte and frame budgets.

## Crates

| Crate | Purpose |
|---|---|
| `telemutter` | Core envelope: parse, write, receiver state machine |
| `telemutter-avr` | AVR entrypoint (atmega2560, `no_std` + `build-std`) |
| `telemutter-cortexm` | Cortex-M entrypoint (thumbv6m, `no_std`) |

## Python

`python/telemutter_py/` is a pure-stdlib mirror of the Rust envelope, used for
host-side tooling and cross-language interop tests.

## Quick start

```bash
# Rust tests (all workspace crates)
cargo test --workspace

# Python tests
python -m unittest discover -s python/tests -v

# Everything (Rust + Python + interop)
python run_everything.py
```

## Documentation

- [Envelope Protocol Spec v0.1.0](docs/PROTOCOL_ENVELOPE_v0.1.0.md) -- normative wire format
- [Test Vectors v0.1.0](docs/TEST_VECTORS_v0.1.0.md) -- golden frames for conformance testing

## License

Apache-2.0
