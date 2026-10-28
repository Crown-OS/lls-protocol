use chacha20poly1305::ChaCha20Poly1305;
use chacha20poly1305::aead::AeadInOut;
use chacha20poly1305::aead::inout::InOutBuf;
use llts_core::clock::Instant;
use llts_wire::StreamId;
use llts_wire::packet::{PacketFlags, PacketHeader, PacketKind, PathProbe, SequenceSpace};

use super::nonce::chacha_nonce;
use super::sending_cipher::TAG_LEN;
use super::{KeyEpoch, KeySchedule, MAX_STREAMS_PER_DIRECTION, ReplayWindow, SequenceExtender};
use crate::TransportError;
use crate::util::BoundedMap;

#[derive(Debug, Default, Clone)]
struct ReceivingSpace {
    extender: SequenceExtender,
    replay: ReplayWindow,
}

/// The two most recent epochs' ciphers for one stream, so a rekey costs one derivation.
#[derive(Debug, Default)]
struct EpochCiphers {
    slots: [Option<(KeyEpoch, ChaCha20Poly1305)>; 2],
}

impl EpochCiphers {
    fn get(&self, epoch: KeyEpoch) -> Option<&ChaCha20Poly1305> {
        self.slots
            .iter()
            .flatten()
            .find_map(|(held, cipher)| (*held == epoch).then_some(cipher))
    }

    fn insert(&mut self, epoch: KeyEpoch, cipher: ChaCha20Poly1305) {
        if self.get(epoch).is_some() {
            return;
        }
        let oldest = self
            .slots
            .iter_mut()
            .min_by_key(|slot| slot.as_ref().map(|(held, _)| *held));
        if let Some(slot) = oldest {
            *slot = Some((epoch, cipher));
        }
    }
}

#[derive(Debug, Default)]
struct ReceivingStream {
    ciphers: EpochCiphers,
    spaces: [ReceivingSpace; SequenceSpace::COUNT],
}

/// Which epoch opened a datagram and at which counter.
#[derive(Debug, Clone, Copy)]
struct Opened {
    epoch: KeyEpoch,
    counter: u64,
}

/// Verification and decryption for the inbound half of a session.
///
/// Opening is `peek → decrypt → replay check → commit`: nothing about the counter moves until the
/// tag has verified, so a forged datagram — however far ahead its sequence claims to be — leaves
/// the receiver exactly where it was. When the first guess fails, one rollover further is tried
/// before giving up, which recovers from a blackout of up to a rollover and a half on its own;
/// longer gaps are closed by the counter records every probe carries.
#[derive(Debug)]
pub struct ReceivingCipher {
    schedule: KeySchedule,
    epoch: KeyEpoch,
    retired_epoch_until: Option<Instant>,
    streams: BoundedMap<StreamId, ReceivingStream, MAX_STREAMS_PER_DIRECTION>,
}

impl ReceivingCipher {
    pub fn new(schedule: KeySchedule) -> Self {
        Self {
            schedule,
            epoch: KeyEpoch::FIRST,
            retired_epoch_until: None,
            streams: BoundedMap::new(),
        }
    }

    pub const fn epoch(&self) -> KeyEpoch {
        self.epoch
    }

    /// Verifies and decrypts in place, returning the plaintext length.
    pub fn open(&mut self, datagram: &mut [u8], now: Instant) -> Result<usize, TransportError> {
        let body_end = datagram
            .len()
            .checked_sub(TAG_LEN)
            .filter(|end| *end >= PacketHeader::ENCODED_LEN)
            .ok_or(TransportError::Unauthenticated)?;
        let (header, _) = PacketHeader::read_from(datagram)?;
        let space =
            SequenceSpace::of(header.kind, header.flags).ok_or(TransportError::NotSealable {
                kind: header.kind.name(),
            })?;

        let opened = self.authenticate(&header, space, datagram, body_end, now)?;
        self.commit(&header, space, opened, now)?;

        if header.kind == PacketKind::Probe {
            self.resync_from_probe(datagram.get(PacketHeader::ENCODED_LEN..body_end));
        }
        Ok(body_end)
    }

    fn authenticate(
        &self,
        header: &PacketHeader,
        space: SequenceSpace,
        datagram: &mut [u8],
        body_end: usize,
        now: Instant,
    ) -> Result<Opened, TransportError> {
        let known = self.streams.get(&header.stream);
        let fresh = ReceivingSpace::default();
        let state = known
            .and_then(|stream| stream.spaces.get(space.index()))
            .unwrap_or(&fresh);

        let guess = state.extender.peek(header.sequence);
        let candidates = [Some(guess), guess.checked_add(SequenceExtender::ROLLOVER)];
        let mut replayed = false;

        for epoch in self.epoch_candidates(header.flags.contains(PacketFlags::KEY_PHASE), now) {
            let derived;
            let cipher = match known.and_then(|stream| stream.ciphers.get(epoch)) {
                Some(cipher) => cipher,
                None => {
                    derived = self.schedule.packet_cipher(header.stream, epoch);
                    &derived
                }
            };
            for counter in candidates.into_iter().flatten() {
                if !state.replay.admits(counter) {
                    replayed = true;
                    continue;
                }
                if open_in_place(cipher, space, counter, datagram, body_end) {
                    return Ok(Opened { epoch, counter });
                }
            }
        }
        Err(if replayed {
            TransportError::Replayed
        } else {
            TransportError::Unauthenticated
        })
    }

    fn commit(
        &mut self,
        header: &PacketHeader,
        space: SequenceSpace,
        opened: Opened,
        now: Instant,
    ) -> Result<(), TransportError> {
        let Self {
            schedule, streams, ..
        } = self;
        let stream = streams
            .get_or_insert_with(header.stream, ReceivingStream::default)
            .ok_or(TransportError::TooManyStreams {
                limit: MAX_STREAMS_PER_DIRECTION,
            })?;
        if stream.ciphers.get(opened.epoch).is_none() {
            stream.ciphers.insert(
                opened.epoch,
                schedule.packet_cipher(header.stream, opened.epoch),
            );
        }
        if let Some(state) = stream.spaces.get_mut(space.index()) {
            state.extender.commit(opened.counter);
            state.replay.record(opened.counter);
        }
        if opened.epoch > self.epoch {
            self.epoch = opened.epoch;
            self.retired_epoch_until = Some(now.saturating_add(KeySchedule::RETIRED_KEY_GRACE));
        }
        Ok(())
    }

    /// The epochs a datagram with this key-phase bit may belong to, most likely first.
    fn epoch_candidates(&self, phase: bool, now: Instant) -> impl Iterator<Item = KeyEpoch> {
        let [first, second] = if phase == self.epoch.phase() {
            [Some(self.epoch), None]
        } else {
            let retired = self
                .retired_epoch_until
                .filter(|until| until.is_after(now))
                .and(self.epoch.previous());
            [retired, self.epoch.next()]
        };
        first.into_iter().chain(second)
    }

    /// Moves every stream the probe names up to the sender's true counter.
    ///
    /// Only reached once the probe has authenticated, so the records are the peer's own word.
    fn resync_from_probe(&mut self, body: Option<&[u8]>) {
        let Some(records) = body.and_then(|body| PathProbe::counter_records(body).ok()) else {
            return;
        };
        for record in records {
            if let Some(state) = self
                .streams
                .get_mut(&record.stream)
                .and_then(|stream| stream.spaces.get_mut(record.space.index()))
            {
                state.extender.commit(record.counter);
            }
        }
    }
}

fn open_in_place(
    cipher: &ChaCha20Poly1305,
    space: SequenceSpace,
    counter: u64,
    datagram: &mut [u8],
    body_end: usize,
) -> bool {
    let (framed, tail) = datagram.split_at_mut(body_end);
    let Some(tag) = tail
        .get(..TAG_LEN)
        .and_then(|bytes| <[u8; TAG_LEN]>::try_from(bytes).ok())
    else {
        return false;
    };
    let (aad, body) = framed.split_at_mut(PacketHeader::ENCODED_LEN);
    cipher
        .decrypt_inout_detached(
            &chacha_nonce(space, counter),
            aad,
            InOutBuf::from(body),
            &tag.into(),
        )
        .is_ok()
}
