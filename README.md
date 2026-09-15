# lls-protocol

A low-latency audio/video streaming protocol for
[CrownOS](https://github.com/Crown-OS) — the transport intended to carry screen
mirroring, second screen and camera sharing between a phone and the desktop.

**Status: Skeleton. It compiles; almost nothing is implemented.** Roughly 119
lines of real code. See [What is a stub](#what-is-a-stub).

## What "lls" stands for

Not stated anywhere in the source. Most likely **Low-Latency Streaming**, based
on the module names (`streaming`, `signaling`, `nvenc`, `packet`, `discovery`,
`reorder_buffer`) and the features it lines up with. *That is an inference.*

Despite the repository description, **no code here references `crowncrate`, and
no code in `crowncrate` references this.** The apparent division of labour is
that `crowncrate` is the low-bandwidth control plane (TCP, CBOR) and this is the
media plane (UDP, RTP-shaped) — but nothing states it.

## Design

UDP transport with an RTP-shaped packet header:

```rust
pub enum PacketType { Audio = 0, Video = 1 }

pub enum FrameRateMode { Low = 30, Standard = 60, Smooth = 90, SuperSmooth = 120 }

pub struct MediaPacket {
    pub packet_type: PacketType,
    /// Spot missing packets.
    pub sequence_number: u16,
    /// Sampling instant of the first octet.
    /// Audio 8 kHz, video 90 kHz. At 60 fps this advances 3000 per frame.
    pub timestamp: u32,
    /// For video: the last packet of a frame — tells the decoder to render.
    pub marker: bool,
    /// Identifies the origin of a media stream.
    pub ssrc: u32,
}
```

Those are the RFC 3550 RTP header fields verbatim, including the standard clock
rates.

`server/src/util/reorder_buffer.rs` is a `BTreeMap<u16, MediaPacket>` jitter
buffer — the only part of the crate with real logic, and only `insert` is
implemented.

## What is a stub

- `client/src/lib.rs` — 0 bytes
- `server/src/{config,signaling,streaming}.rs` — 0 bytes
  (`streaming.rs` is also not declared in `lib.rs`)
- `server/src/nvenc/mod.rs`, `server/src/platform/mod.rs` — 0 bytes
- Both `main.rs` files — `println!("Hello, world!")`
- `MediaPacket::parse()` — empty, private, no arguments
- `DiscoveredDevice::discover()` — always returns an empty `Vec`
- `Connection::get_byte_stream()` — empty
- `serde` is declared and nothing derives it

`client` does not depend on `server`, and all shared types live inside `server` —
so for a repository named "protocol", there is currently no shared protocol
crate.

## Suggested order of work

1. Move `packet`, `protocol` and `discovery` into a shared crate that both
   `client` and `server` depend on. That crate is also what would claim the
   `lls-protocol` name — today the repository name corresponds to no crate.
2. Implement `MediaPacket::parse` and a matching serialiser.
3. Complete the reorder buffer — it needs a drain with a playout deadline.

The two items that used to head this list are done: the root is now a virtual
workspace with a real `[workspace.dependencies]` table, and `Connection::connect`
binds with `UdpSocket::bind` instead of the `UdpSocket::try_from(IpAddr)` that
never compiled.

## Build and run

```bash
cargo build
cargo run -p lls-server      # prints "Hello, world!"
cargo run -p lls-client      # prints "Hello, world!"
```

Rust 1.88+ (pinned in `rust-toolchain.toml`). No native or CrownOS dependencies,
so no dev overlay is needed. There are no tests.

Nothing is publishable yet: `client/src/lib.rs` and four of the server's modules
are zero-byte files, so a release would ship empty crates.

## Contributing

See the organization-wide
[contribution guide](https://github.com/Crown-OS/crownOs/blob/main/CONTRIBUTING.md).
Default branch here is **`main`**.

## License

Licensed under the [MIT License](LICENSE).
