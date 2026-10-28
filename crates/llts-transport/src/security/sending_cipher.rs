use chacha20poly1305::ChaCha20Poly1305;
use chacha20poly1305::aead::AeadInOut;
use chacha20poly1305::aead::inout::InOutBuf;
use llts_core::clock::Instant;
use llts_wire::StreamId;
use llts_wire::packet::{
    CounterRecord, PacketFlags, PacketHeader, PacketKind, PathProbe, SequenceSpace,
};

use super::nonce::chacha_nonce;
use super::{KeyEpoch, KeySchedule, MAX_STREAMS_PER_DIRECTION, SealedDatagram, SequenceExtender};
use crate::TransportError;
use crate::util::BoundedMap;

/// Authentication tag appended to every sealed datagram.
pub const TAG_LEN: usize = 16;

#[derive(Debug)]
struct SendingStream {
    epoch: KeyEpoch,
    cipher: ChaCha20Poly1305,
    counters: [SequenceExtender; SequenceSpace::COUNT],
}

/// Authenticated encryption for the outbound half of a session.
///
/// ChaCha20-Poly1305 rather than AES-GCM by default: phones without an AES instruction set run it
/// several times faster, and neither side is close to CPU-bound either way.
///
/// The header travels in the clear but authenticated as additional data. The nonce is rebuilt
/// from it — the sequence space from kind and flags, the counter from the sequence — so it never
/// travels, and overhead is the 16-byte tag alone. Every `(stream, space)` keeps its own counter,
/// and sealing refuses any sequence that does not move that counter forward: a nonce reused
/// under one key leaks the Poly1305 key, so this is checked here rather than trusted upstream.
#[derive(Debug)]
pub struct SendingCipher {
    schedule: KeySchedule,
    epoch: KeyEpoch,
    epoch_started_at: Option<Instant>,
    sealed_in_epoch: u64,
    streams: BoundedMap<StreamId, SendingStream, MAX_STREAMS_PER_DIRECTION>,
}

impl SendingCipher {
    pub fn new(schedule: KeySchedule) -> Self {
        Self {
            schedule,
            epoch: KeyEpoch::FIRST,
            epoch_started_at: None,
            sealed_in_epoch: 0,
            streams: BoundedMap::new(),
        }
    }

    pub const fn epoch(&self) -> KeyEpoch {
        self.epoch
    }

    /// Moves to the next key epoch now, regardless of how much the current one has sealed.
    pub const fn rekey(&mut self, now: Instant) {
        if let Some(next) = self.epoch.next() {
            self.epoch = next;
            self.epoch_started_at = Some(now);
            self.sealed_in_epoch = 0;
        }
    }

    /// Every counter this side has sealed with, for a probe to carry to the peer.
    pub fn counter_records(&self) -> impl Iterator<Item = CounterRecord> + '_ {
        self.streams.iter().flat_map(|(stream, state)| {
            SequenceSpace::ALL
                .into_iter()
                .zip(state.counters)
                .filter_map(|(space, extender)| {
                    extender.highest().map(|counter| CounterRecord {
                        stream: *stream,
                        space,
                        counter,
                    })
                })
        })
    }

    /// Encrypts the body in place and writes the tag directly after it.
    ///
    /// `datagram` holds the header followed by `body_len` body bytes and must have at least
    /// [`TAG_LEN`] further bytes of room. The key-phase bit is set here, and a probe gets the
    /// sender's counter records written into whatever room its buffer has.
    pub fn seal(
        &mut self,
        datagram: &mut [u8],
        body_len: usize,
        now: Instant,
    ) -> Result<SealedDatagram, TransportError> {
        let (header, _) = PacketHeader::read_from(datagram)?;
        let space =
            SequenceSpace::of(header.kind, header.flags).ok_or(TransportError::NotSealable {
                kind: header.kind.name(),
            })?;
        self.rotate_if_due(now);
        set_key_phase(datagram, self.epoch)?;

        let body_len = if header.kind == PacketKind::Probe {
            self.write_counter_records(datagram, body_len)?
        } else {
            body_len
        };
        let body_end = PacketHeader::ENCODED_LEN + body_len;
        let sealed_len = body_end + TAG_LEN;
        if datagram.len() < sealed_len {
            return Err(TransportError::DatagramTooLarge {
                len: sealed_len,
                limit: datagram.len(),
            });
        }

        let Self {
            schedule,
            epoch,
            streams,
            ..
        } = self;
        let state = streams
            .get_or_insert_with(header.stream, || SendingStream {
                epoch: *epoch,
                cipher: schedule.packet_cipher(header.stream, *epoch),
                counters: [SequenceExtender::new(); SequenceSpace::COUNT],
            })
            .ok_or(TransportError::TooManyStreams {
                limit: MAX_STREAMS_PER_DIRECTION,
            })?;
        if state.epoch != *epoch {
            state.cipher = schedule.packet_cipher(header.stream, *epoch);
            state.epoch = *epoch;
        }
        let extender =
            state
                .counters
                .get_mut(space.index())
                .ok_or(TransportError::NotSealable {
                    kind: header.kind.name(),
                })?;
        let mut counter = extender.peek(header.sequence);
        if let Some(highest) = extender.highest()
            && counter <= highest
        {
            // A stream closed and opened again under the same id starts its wire sequence
            // over; its counters continue one rollover on, where the receiver's retry finds them.
            if header.sequence.get() != 0 {
                return Err(TransportError::SequenceRegressed {
                    stream: header.stream,
                    space,
                });
            }
            counter = (highest / SequenceExtender::ROLLOVER + 1) * SequenceExtender::ROLLOVER;
        }

        let (framed, tail) = datagram.split_at_mut(body_end);
        let (aad, body) = framed.split_at_mut(PacketHeader::ENCODED_LEN);
        let tag = state
            .cipher
            .encrypt_inout_detached(&chacha_nonce(space, counter), aad, InOutBuf::from(body))
            .map_err(|_| TransportError::Unauthenticated)?;
        tail.get_mut(..TAG_LEN)
            .ok_or(TransportError::Unauthenticated)?
            .copy_from_slice(&tag);

        extender.commit(counter);
        self.sealed_in_epoch = self.sealed_in_epoch.saturating_add(1);
        Ok(SealedDatagram {
            len: sealed_len,
            stream: header.stream,
            epoch: self.epoch,
            space,
            counter,
        })
    }

    fn rotate_if_due(&mut self, now: Instant) {
        let started = *self.epoch_started_at.get_or_insert(now);
        if now.saturating_elapsed_since(started) >= KeySchedule::REKEY_INTERVAL
            || self.sealed_in_epoch >= KeySchedule::REKEY_PACKET_LIMIT
        {
            self.rekey(now);
        }
    }

    /// Fills a probe body with counter records, as far as the buffer's room allows, and returns
    /// the body length that results.
    fn write_counter_records(
        &self,
        datagram: &mut [u8],
        body_len: usize,
    ) -> Result<usize, TransportError> {
        if body_len < PathProbe::BODY_LEN {
            return Ok(body_len);
        }
        let room = datagram.len().saturating_sub(TAG_LEN);
        let body = datagram.get_mut(PacketHeader::ENCODED_LEN..room).ok_or(
            TransportError::DatagramTooLarge {
                len: PacketHeader::ENCODED_LEN + body_len + TAG_LEN,
                limit: room,
            },
        )?;
        let reached = PathProbe::write_counter_records(body, self.counter_records())?;
        Ok(reached.max(body_len))
    }
}

pub(crate) fn set_key_phase(datagram: &mut [u8], epoch: KeyEpoch) -> Result<(), TransportError> {
    let flags = datagram
        .get_mut(PacketHeader::FLAGS_OFFSET)
        .ok_or(TransportError::Unauthenticated)?;
    let mut parsed = PacketFlags::from_bits_retain(*flags);
    parsed.set(PacketFlags::KEY_PHASE, epoch.phase());
    *flags = parsed.bits();
    Ok(())
}
