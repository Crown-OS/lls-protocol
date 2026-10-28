# llts-protocol

Low-latency video transport for CrownOS continuity: a Linux laptop and an Android device sharing a
screen or a camera over LAN Wi-Fi or USB, in either direction.

## Targets

| Metric | Target |
| --- | --- |
| Protocol latency, frame in to frame out | ≤ 12 ms Wi-Fi, ≤ 8 ms USB |
| Glass-to-glass, Linux screen to Android display | ≤ 40 ms at 120 Hz |
| Glass-to-glass, Android camera to Linux | ≈ 55 ms, ~30 ms of it camera hardware |

Only the first is a CI gate; the other two need a photodiode rig.

## Crates

| Crate | Role |
| --- | --- |
| `llts-wire` | Byte layout. `no_std`, allocation-free, fuzzable in isolation. |
| `llts-core` | Sans-I/O protocol engine: packetise, repair, congestion control, playout. |
| `llts-media` | Capture, encode, decode and present, with per-vendor Linux backends. |
| `llts-transport` | UDP, QUIC and USB drivers, plus the Noise handshake. |
| `llts-android` | JNI bridge for the Android app. |
| `llts-signaling` | Device discovery and the control-action vocabulary. |
| `llts-node` | Sans-I/O orchestrator: paired peers, pairing, autoconnect, state sync, feature negotiation. |

## Working on it

```sh
just              # lint + test
just lint         # clippy with -D warnings, plus a rustfmt check
just bench        # loopback latency measurement, exits non-zero if over budget
just bench-sweep  # the same across the loss rates the repair policy covers
just android      # build the .so into the Gradle module
```

## Measured today

`llts-bench` runs two protocol engines over real loopback sockets with a synthetic encoder, at
1080p-scale frames (25 kB, 60 Hz, 12 Mbit/s). Protocol latency is encode-submit to
frame-presented, p99 over 300 frames:

| Injected loss | p99 | Frames delivered | Repair used |
| --- | --- | --- | --- |
| none | 4.9 ms | 300 / 300 | none — no parity, no retransmits |
| 1 % | 5.4 ms | 300 / 300 | 60 fragments from parity |
| 2 % | 5.4 ms | 300 / 300 | 159 fragments from parity |
| 5 % | 6.3 ms | 300 / 300 | 300 fragments from parity |

The clean-link row is the one that matters most: the adaptive policy spends nothing at all when
the link is quiet, which is the whole reason it is not a fixed rate.

`unwrap` and `expect` are denied outside tests, `unsafe` is confined to the GPU, batch-I/O and FFI
modules, and every library crate exposes exactly one `thiserror` enum. `anyhow` belongs in
`llts-bench` and examples only.
