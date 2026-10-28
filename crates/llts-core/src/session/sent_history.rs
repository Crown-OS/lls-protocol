use alloc::boxed::Box;
use alloc::vec;

use llts_wire::{MonotonicMicros, SequenceNumber};

/// What the sender remembers about one paced packet until the peer reports its arrival.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SentPacket {
    pub transport_sequence: SequenceNumber,
    pub departure: MonotonicMicros,
    pub byte_len: u32,
}

/// Departure records for recent paced packets, indexed by transport sequence.
///
/// A ring rather than a map: the index is the sequence modulo the ring, and each slot remembers
/// which sequence it holds, so a stale or duplicated report entry simply misses.
#[derive(Debug)]
pub struct SentHistory {
    slots: Box<[Option<SentPacket>]>,
}

impl Default for SentHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl SentHistory {
    /// Enough for a quarter second at 9 000 packets a second, several times any report delay.
    pub const CAPACITY: usize = 2_048;

    pub fn new() -> Self {
        Self {
            slots: vec![None; Self::CAPACITY].into_boxed_slice(),
        }
    }

    pub fn record(&mut self, packet: SentPacket) {
        if let Some(slot) = self.slots.get_mut(Self::index(packet.transport_sequence)) {
            *slot = Some(packet);
        }
    }

    /// Removes and returns the record for `transport_sequence`, so a duplicated report entry
    /// cannot count one packet twice.
    pub fn take(&mut self, transport_sequence: SequenceNumber) -> Option<SentPacket> {
        let slot = self.slots.get_mut(Self::index(transport_sequence))?;
        if slot.is_some_and(|packet| packet.transport_sequence == transport_sequence) {
            slot.take()
        } else {
            None
        }
    }

    pub fn clear(&mut self) {
        self.slots.fill(None);
    }

    fn index(transport_sequence: SequenceNumber) -> usize {
        usize::from(transport_sequence.get()) % Self::CAPACITY
    }
}
