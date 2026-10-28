use alloc::vec::Vec;

use llts_wire::SequenceNumber;
use llts_wire::packet::MissingRange;

/// Extends the trailing range when `sequence` continues it, otherwise starts a new one.
///
/// Losses cluster, because a Wi-Fi aggregate drops as a unit, so a whole burst usually collapses
/// into a single three-byte range.
pub(crate) fn append_coalescing(ranges: &mut Vec<MissingRange>, sequence: SequenceNumber) {
    if let Some(last) = ranges.last_mut()
        && last.extra < u8::MAX
        && last.first.advanced_by(u16::from(last.extra)).next() == sequence
    {
        last.extra = last.extra.saturating_add(1);
        return;
    }
    ranges.push(MissingRange::single(sequence));
}
