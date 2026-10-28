use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::CoreError;
use crate::channel::{InputChannel, ReliableChannel};
use crate::stream::{MediaStream, StreamKey};

/// What one open stream is.
#[derive(Debug)]
pub(crate) enum StreamChannel {
    Media(Box<MediaStream>),
    Reliable(Box<ReliableChannel>),
    Input(Box<InputChannel>),
}

/// The streams open to one peer. Anything not in here is refused on arrival.
#[derive(Debug)]
pub(crate) struct StreamTable {
    entries: Vec<(StreamKey, StreamChannel)>,
}

impl StreamTable {
    pub(crate) const MAX_STREAMS: usize = 16;

    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::with_capacity(Self::MAX_STREAMS),
        }
    }

    pub(crate) fn insert(
        &mut self,
        key: StreamKey,
        channel: StreamChannel,
    ) -> Result<(), CoreError> {
        if self.get(key).is_some() {
            return Err(CoreError::StreamAlreadyOpen {
                class: key.class.name(),
                index: key.index,
            });
        }
        if self.entries.len() >= Self::MAX_STREAMS {
            return Err(CoreError::TooManyStreams {
                limit: Self::MAX_STREAMS,
            });
        }
        self.entries.push((key, channel));
        Ok(())
    }

    pub(crate) fn remove(&mut self, key: StreamKey) -> Option<StreamChannel> {
        let position = self.entries.iter().position(|(held, _)| *held == key)?;
        Some(self.entries.swap_remove(position).1)
    }

    pub(crate) fn get(&self, key: StreamKey) -> Option<&StreamChannel> {
        self.entries
            .iter()
            .find_map(|(held, channel)| (*held == key).then_some(channel))
    }

    pub(crate) fn get_mut(&mut self, key: StreamKey) -> Option<&mut StreamChannel> {
        self.entries
            .iter_mut()
            .find_map(|(held, channel)| (*held == key).then_some(channel))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &StreamChannel> {
        self.entries.iter().map(|(_, channel)| channel)
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut StreamChannel> {
        self.entries.iter_mut().map(|(_, channel)| channel)
    }

    pub(crate) fn media(&self) -> impl Iterator<Item = &MediaStream> {
        self.iter().filter_map(|channel| match channel {
            StreamChannel::Media(stream) => Some(stream.as_ref()),
            StreamChannel::Reliable(_) | StreamChannel::Input(_) => None,
        })
    }
}
