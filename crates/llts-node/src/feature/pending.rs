use llts_core::clock::Instant;
use llts_signaling::device::Feature;

use crate::feature::FeatureRequest;
use crate::feature::active::MEDIA_FEATURES;

/// A start asked for while the session was up but the peer had not announced what it supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingStart {
    pub(crate) request: FeatureRequest,
    pub(crate) expires_at: Instant,
}

/// The starts of one session waiting for the peer's capabilities, at most one per feature, so
/// the queue is bounded by the number of media features.
#[derive(Debug, Default)]
pub(crate) struct PendingStarts {
    entries: heapless::Vec<PendingStart, MEDIA_FEATURES>,
}

impl PendingStarts {
    /// Queues `start`, replacing an earlier request for the same feature.
    pub(crate) fn queue(&mut self, start: PendingStart) {
        self.cancel(start.request.feature());
        let _ = self.entries.push(start);
    }

    pub(crate) fn cancel(&mut self, feature: Feature) -> Option<PendingStart> {
        let position = self
            .entries
            .iter()
            .position(|entry| entry.request.feature() == feature)?;
        Some(self.entries.swap_remove(position))
    }

    pub(crate) fn drain(&mut self) -> impl Iterator<Item = PendingStart> + use<> {
        core::mem::take(&mut self.entries).into_iter()
    }

    /// Removes and returns every start whose wait ran out by `now`.
    pub(crate) fn take_expired(
        &mut self,
        now: Instant,
    ) -> impl Iterator<Item = PendingStart> + use<> {
        let (expired, waiting) = core::mem::take(&mut self.entries)
            .into_iter()
            .partition::<heapless::Vec<_, MEDIA_FEATURES>, _>(|entry| {
                !entry.expires_at.is_after(now)
            });
        self.entries = waiting;
        expired.into_iter()
    }

    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.entries.iter().map(|entry| entry.expires_at).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(request: FeatureRequest, at_micros: u64) -> PendingStart {
        PendingStart {
            request,
            expires_at: Instant::from_micros(at_micros),
        }
    }

    #[test]
    fn a_feature_waits_once_with_its_latest_request() {
        let mut pending = PendingStarts::default();
        pending.queue(start(FeatureRequest::Mic, 10));
        pending.queue(start(FeatureRequest::Mic, 20));
        assert_eq!(pending.next_deadline(), Some(Instant::from_micros(20)));
        assert_eq!(pending.drain().count(), 1);
        assert_eq!(pending.next_deadline(), None);
    }

    #[test]
    fn only_starts_whose_wait_ran_out_expire() {
        let mut pending = PendingStarts::default();
        pending.queue(start(FeatureRequest::Mic, 10));
        assert_eq!(pending.take_expired(Instant::from_micros(9)).count(), 0);
        assert_eq!(
            pending
                .take_expired(Instant::from_micros(10))
                .map(|entry| entry.request)
                .collect::<Vec<_>>(),
            [FeatureRequest::Mic]
        );
        assert!(pending.cancel(Feature::Mic).is_none());
    }
}
