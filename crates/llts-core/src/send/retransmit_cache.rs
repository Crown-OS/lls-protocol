use alloc::collections::VecDeque;

use llts_wire::packet::PacketFlags;
use llts_wire::{MonotonicMicros, SequenceNumber};

use crate::CoreError;
use crate::buffer::{BufferPool, Datagram};

/// Holds recently sent datagrams so a NACK can be answered without re-encoding.
///
/// Bounded by bytes rather than by count, because that is what the memory budget cares about and
/// because a keyframe's fragments are the ones worth keeping longest. Entries leave from the
/// front, so the cache always covers the most recent window of the sequence stream.
#[derive(Debug)]
pub struct RetransmitCache {
    entries: VecDeque<Datagram>,
    oldest: Option<SequenceNumber>,
    cached_bytes: usize,
    capacity_bytes: usize,
    evicted: u64,
    served: u64,
}

impl RetransmitCache {
    pub fn new(capacity_bytes: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(64),
            oldest: None,
            cached_bytes: 0,
            capacity_bytes,
            evicted: 0,
            served: 0,
        }
    }

    pub const fn cached_bytes(&self) -> usize {
        self.cached_bytes
    }

    pub fn cached_datagrams(&self) -> usize {
        self.entries.len()
    }

    pub const fn evicted(&self) -> u64 {
        self.evicted
    }

    pub const fn served(&self) -> u64 {
        self.served
    }

    pub const fn oldest_cached(&self) -> Option<SequenceNumber> {
        self.oldest
    }

    /// Takes ownership of a datagram that has just been handed to the link.
    ///
    /// A sequence that does not continue the cached run means the sender restarted its numbering,
    /// so the old contents can no longer be addressed and are dropped.
    pub fn store(&mut self, datagram: Datagram) {
        let sequence = datagram.header().sequence;
        if self.next_expected().is_some_and(|next| next != sequence) {
            self.clear();
        }
        if self.entries.is_empty() {
            self.oldest = Some(sequence);
        }
        self.cached_bytes = self.cached_bytes.saturating_add(datagram.byte_len());
        self.entries.push_back(datagram);
        self.evict_to_capacity();
    }

    /// Records when a cached datagram actually left, so a later acknowledgement measures the
    /// path rather than the pacer queue it sat in.
    pub fn restamp(&mut self, sequence: SequenceNumber, departure: MonotonicMicros) {
        let Some(position) = self.position_of(sequence) else {
            return;
        };
        let Some(entry) = self.entries.get_mut(position) else {
            return;
        };
        let mut header = *entry.header();
        header.departure = departure;
        let _ = entry.rewrite_header(&header);
    }

    pub fn contains(&self, sequence: SequenceNumber) -> bool {
        self.position_of(sequence).is_some()
    }

    /// When a cached datagram left, and how large it was.
    ///
    /// The congestion controller needs both to turn an acknowledgement into a delay sample, and
    /// the cache is already the one place that remembers them.
    pub fn departure_of(&self, sequence: SequenceNumber) -> Option<(MonotonicMicros, u32)> {
        let entry = self.entries.get(self.position_of(sequence)?)?;
        let byte_len = u32::try_from(entry.byte_len()).unwrap_or(u32::MAX);
        Some((entry.header().departure, byte_len))
    }

    /// Builds a fresh datagram carrying the cached payload, stamped as a retransmission and
    /// re-dated so the receiver's delay estimator reads it as the recent send that it is.
    ///
    /// The rebuilt header takes `retransmission` as its sequence and carries the original in the
    /// encrypted body prefix. Keeping the original sequence in the header would seal new
    /// additional data under a nonce already spent, which hands an observer the Poly1305 key.
    ///
    /// The cached copy stays put, because a second NACK for the same sequence is routine when the
    /// first request was itself lost.
    pub fn rebuild(
        &mut self,
        sequence: SequenceNumber,
        retransmission: SequenceNumber,
        departure: MonotonicMicros,
        pool: &BufferPool,
    ) -> Result<Option<Datagram>, CoreError> {
        let Some(position) = self.position_of(sequence) else {
            return Ok(None);
        };
        let Some(cached) = self.entries.get(position) else {
            return Ok(None);
        };

        let mut header = *cached.header();
        header.flags |= PacketFlags::RETRANSMISSION;
        header.sequence = retransmission;
        header.departure = departure;

        let rebuilt = Datagram::assemble_retransmission(&header, sequence, cached.payload(), pool)?;
        self.served = self.served.saturating_add(1);
        Ok(Some(rebuilt))
    }

    /// Releases everything up to and including `sequence`, which the sender does once feedback
    /// shows the receiver has it.
    pub fn release_through(&mut self, sequence: SequenceNumber) {
        while let Some(front) = self.entries.front() {
            let cached = front.header().sequence;
            if cached != sequence && !cached.precedes(sequence) {
                break;
            }
            self.pop_front();
        }
    }

    /// Releases everything built more than `retention_micros` before `now`.
    ///
    /// Age rather than acknowledgement decides, because a cumulative acknowledgement cannot tell
    /// a sequence that arrived from one still waiting behind a gap: releasing through the
    /// highest acknowledged sequence would free exactly the datagram a NACK is about to ask for.
    pub fn release_older_than(&mut self, now: MonotonicMicros, retention_micros: u32) {
        while let Some(front) = self.entries.front() {
            if now.elapsed_since(front.header().departure) <= retention_micros {
                break;
            }
            self.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.oldest = None;
        self.cached_bytes = 0;
    }

    fn next_expected(&self) -> Option<SequenceNumber> {
        self.entries
            .back()
            .map(|last| last.header().sequence.next())
    }

    fn position_of(&self, sequence: SequenceNumber) -> Option<usize> {
        let offset = self.oldest?.distance_to(sequence);
        let offset = usize::try_from(offset).ok()?;
        (offset < self.entries.len()).then_some(offset)
    }

    fn evict_to_capacity(&mut self) {
        while self.cached_bytes > self.capacity_bytes && self.entries.len() > 1 {
            self.pop_front();
            self.evicted = self.evicted.saturating_add(1);
        }
    }

    fn pop_front(&mut self) {
        let Some(datagram) = self.entries.pop_front() else {
            return;
        };
        self.cached_bytes = self.cached_bytes.saturating_sub(datagram.byte_len());
        self.oldest = self.entries.front().map(|next| next.header().sequence);
    }
}
