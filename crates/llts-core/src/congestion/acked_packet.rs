use llts_wire::MonotonicMicros;

/// One packet the receiver confirmed, with both endpoints' timestamps for it.
///
/// The two clocks need no common origin: every estimate here is built from *differences* between
/// consecutive packets, so a constant offset cancels out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AckedPacket {
    pub departure: MonotonicMicros,
    pub arrival: MonotonicMicros,
    pub byte_len: u32,
}
