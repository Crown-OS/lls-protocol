use core::time::Duration;

/// Microseconds in `span`, clamped instead of wrapped.
///
/// A `Duration` spans more microseconds than a `u64` holds, and `Duration::as_micros` returns a
/// `u128` that cannot be narrowed inside a `const fn` on stable. Building the value from seconds
/// and the sub-second remainder avoids both problems.
#[allow(
    clippy::cast_lossless,
    reason = "u32 to u64 is widening, and `u64::from` is not const on stable"
)]
pub(crate) const fn saturating_micros(span: Duration) -> u64 {
    span.as_secs()
        .saturating_mul(1_000_000)
        .saturating_add(span.subsec_micros() as u64)
}
