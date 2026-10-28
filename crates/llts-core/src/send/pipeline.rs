use alloc::vec::Vec;

use llts_wire::packet::{MissingRange, PacketFlags, PacketHeader, PacketKind, RepairBlockGeometry};
use llts_wire::{MonotonicMicros, ProtocolVersion, SequenceNumber, StreamId};

use super::{DatagramSizing, EmittedRange, EncodedSlice, FramePacketizer, RetransmitCache};
use crate::CoreError;
use crate::buffer::{BufferPool, Datagram};
use crate::repair::{RepairBlockEncoder, RepairPlan, ShardLayout};

/// One stream's sending half: fragment, protect, and keep what a NACK might ask for.
///
/// Composed rather than layered so that sequence numbering stays in one place — parity shards
/// share the media stream's counter, which is what lets the receiver's gap tracker see a single
/// contiguous run and notice a lost parity shard the same way it notices a lost fragment.
///
/// Pacing is not done here: every stream to a peer shares one pacer, so datagrams wait in
/// [`SendPipeline::drain_outgoing`] for the session to hand them over.
#[derive(Debug)]
pub struct SendPipeline {
    packetizer: FramePacketizer,
    parity: RepairBlockEncoder,
    cache: RetransmitCache,
    pool: BufferPool,
    /// Numbering for retransmissions, a separate space from the media counter so a resent
    /// fragment is sealed under a nonce no packet has used before.
    next_retransmission: SequenceNumber,
    staged: Vec<Datagram>,
    outgoing: Vec<Datagram>,
    geometry_scratch: Vec<u8>,
}

impl SendPipeline {
    pub fn new(
        stream: StreamId,
        sizing: DatagramSizing,
        pool: BufferPool,
        cache_capacity_bytes: usize,
    ) -> Self {
        Self {
            packetizer: FramePacketizer::new(stream, sizing, pool.clone()),
            parity: RepairBlockEncoder::new(),
            cache: RetransmitCache::new(cache_capacity_bytes),
            pool,
            next_retransmission: SequenceNumber::ZERO,
            staged: Vec::with_capacity(64),
            outgoing: Vec::with_capacity(64),
            geometry_scratch: alloc::vec![0; RepairBlockGeometry::ENCODED_LEN],
        }
    }

    pub const fn sizing(&self) -> DatagramSizing {
        self.packetizer.sizing()
    }

    /// Fragments a slice, appends its parity shards, and queues the lot for the pacer.
    pub fn submit_slice(
        &mut self,
        slice: EncodedSlice<'_>,
        plan: &RepairPlan,
        departure: MonotonicMicros,
    ) -> Result<EmittedRange, CoreError> {
        self.staged.clear();
        let range = self
            .packetizer
            .packetize(slice, departure, &mut self.staged)?;

        if plan.parity_shards > 0 {
            self.append_parity(&range, plan.parity_shards, departure)?;
        }
        for datagram in self.staged.drain(..) {
            if !datagram.header().is_discardable() {
                let retained = datagram.duplicate(&self.pool)?;
                self.cache.store(retained);
            }
            self.outgoing.push(datagram);
        }
        Ok(range)
    }

    /// Datagrams built since the last call, in the order they must leave.
    pub fn drain_outgoing(&mut self) -> impl Iterator<Item = Datagram> + '_ {
        self.outgoing.drain(..)
    }

    /// Forgets cached datagrams built more than `retention` micros before `now`, which no NACK
    /// could still usefully ask for.
    pub fn release_older_than(&mut self, now: MonotonicMicros, retention_micros: u32) {
        self.cache.release_older_than(now, retention_micros);
    }

    /// Rebuilds every cached datagram the peer asked for and queues it for the pacer.
    pub fn answer_nack(
        &mut self,
        ranges: impl Iterator<Item = MissingRange>,
        departure: MonotonicMicros,
    ) -> Result<u16, CoreError> {
        let mut answered = 0;
        for range in ranges {
            for sequence in range.sequences() {
                if !self.cache.contains(sequence) {
                    continue;
                }
                let retransmission = self.next_retransmission;
                if let Some(datagram) =
                    self.cache
                        .rebuild(sequence, retransmission, departure, &self.pool)?
                {
                    self.next_retransmission = retransmission.next();
                    self.outgoing.push(datagram);
                    answered += 1;
                }
            }
        }
        Ok(answered)
    }

    /// Frees cached datagrams the peer has confirmed receiving.
    pub fn release_through(&mut self, sequence: SequenceNumber) {
        self.cache.release_through(sequence);
    }

    pub const fn cache(&self) -> &RetransmitCache {
        &self.cache
    }

    fn append_parity(
        &mut self,
        range: &EmittedRange,
        parity_shards: u16,
        departure: MonotonicMicros,
    ) -> Result<(), CoreError> {
        let shard_bytes =
            u16::try_from(self.packetizer.sizing().max_payload_len()).unwrap_or(u16::MAX);
        let layout = ShardLayout::new(range.byte_len, shard_bytes)?;

        let Self {
            packetizer,
            parity,
            pool,
            staged,
            geometry_scratch,
            ..
        } = self;

        parity.begin(layout, parity_shards)?;
        for datagram in staged.iter() {
            parity.add_data_shard(datagram.payload())?;
        }

        let geometry = RepairBlockGeometry {
            first_data_index: range.first_fragment_index,
            data_shard_count: range.fragment_count,
            shard_bytes,
            block_byte_offset: range.byte_offset,
            block_byte_len: range.byte_len,
        };
        geometry.write_into(geometry_scratch)?;

        let shards = parity.encode()?;
        let total_shards = range.fragment_count.saturating_add(shards.count());

        for (position, shard) in shards.iter().enumerate() {
            let index = u16::try_from(position).unwrap_or(u16::MAX);
            let header = PacketHeader {
                version: ProtocolVersion::CURRENT,
                kind: PacketKind::Repair,
                flags: if range.frame_closed {
                    PacketFlags::FRAME_END
                } else {
                    PacketFlags::empty()
                },
                stream: packetizer.stream(),
                sequence: packetizer.take_sequence(),
                frame: range.frame,
                timestamp: packetizer.open_frame_timestamp(),
                packet_index: range.fragment_count.saturating_add(index),
                packet_count: total_shards,
                departure,
                payload_offset: range.byte_offset,
            };
            staged.push(Datagram::assemble(
                &header,
                &[geometry_scratch, shard],
                pool,
            )?);
        }
        Ok(())
    }
}
