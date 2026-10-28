use core::time::Duration;

use super::{AdaptivePlayoutDelay, AssembledFrame};
use crate::clock::Instant;
use crate::units::FrameRate;
use crate::util::duration::saturating_micros;

/// Holds the freshest completed frame until its presentation instant.
///
/// One slot, not a queue. If a newer frame completes before the held one has been shown, the held
/// one is already worthless: nobody wants to watch a stale frame in order to watch a fresh one a
/// moment later. Moonlight offers this as "prefer lowest latency" and scrcpy makes it the only
/// behaviour; both are right for an interactive link.
#[derive(Debug)]
pub struct PlayoutBuffer {
    held: Option<AssembledFrame>,
    present_at: Instant,
    delay: AdaptivePlayoutDelay,
    cadence: Duration,
    max_age: Duration,
    last_completion: Option<Instant>,
    superseded: u64,
    expired: u64,
    presented: u64,
}

impl PlayoutBuffer {
    /// A frame older than this many cadences has missed its moment entirely.
    const MAX_AGE_IN_FRAMES: u32 = 2;

    /// Hold time is capped at a quarter of a frame period.
    ///
    /// A jitter buffer sized to a whole period is self-defeating on an interactive link: the next
    /// frame completes while the held one is still waiting, supersedes it, and the viewer has
    /// paid a full frame of latency to watch a frame that was then discarded. A quarter period
    /// absorbs ordinary aggregation jitter and nothing more.
    const MAX_HOLD_DIVISOR: u32 = 4;

    pub const fn new(rate: FrameRate) -> Self {
        let cadence = rate.period();
        Self {
            held: None,
            present_at: Instant::SESSION_START,
            delay: AdaptivePlayoutDelay::new(hold_ceiling(cadence)),
            cadence,
            max_age: cadence.saturating_mul(Self::MAX_AGE_IN_FRAMES),
            last_completion: None,
            superseded: 0,
            expired: 0,
            presented: 0,
        }
    }

    pub fn set_frame_rate(&mut self, rate: FrameRate) {
        self.cadence = rate.period();
        self.max_age = self.cadence.saturating_mul(Self::MAX_AGE_IN_FRAMES);
        self.delay.set_ceiling(hold_ceiling(self.cadence));
    }

    pub const fn target_delay(&self) -> Duration {
        self.delay.target()
    }

    /// Frames dropped because a newer one completed first.
    pub const fn superseded(&self) -> u64 {
        self.superseded
    }

    /// Frames dropped because they completed too late to be worth showing.
    pub const fn expired(&self) -> u64 {
        self.expired
    }

    pub const fn presented(&self) -> u64 {
        self.presented
    }

    pub const fn holds_frame(&self) -> bool {
        self.held.is_some()
    }

    pub fn accept(&mut self, frame: AssembledFrame, now: Instant) {
        self.observe_cadence(frame.completed_at());

        if now.saturating_elapsed_since(frame.completed_at()) > self.max_age {
            self.expired = self.expired.saturating_add(1);
            return;
        }
        if let Some(held) = &self.held {
            if frame.frame().precedes(held.frame()) {
                self.expired = self.expired.saturating_add(1);
                return;
            }
            self.superseded = self.superseded.saturating_add(1);
        }

        self.present_at = frame.completed_at().saturating_add(self.delay.target());
        self.held = Some(frame);
    }

    /// Releases the held frame once its presentation instant has arrived.
    pub fn poll_present(&mut self, now: Instant) -> Option<AssembledFrame> {
        if self.present_at.is_after(now) {
            return None;
        }
        let frame = self.held.take()?;
        self.presented = self.presented.saturating_add(1);
        Some(frame)
    }

    pub fn next_presentation(&self) -> Option<Instant> {
        self.held.as_ref().map(|_| self.present_at)
    }

    fn observe_cadence(&mut self, completed_at: Instant) {
        if let Some(previous) = self.last_completion {
            // Only lateness needs absorbing. A frame that completes early is already waiting, so
            // counting its earliness as jitter would buy buffering nobody needs.
            let gap = completed_at.saturating_elapsed_since(previous);
            self.delay.observe(gap.saturating_sub(self.cadence));
        }
        self.last_completion = Some(completed_at);
    }
}

const fn hold_ceiling(cadence: Duration) -> Duration {
    Duration::from_micros(saturating_micros(cadence) / PlayoutBuffer::MAX_HOLD_DIVISOR as u64)
}
