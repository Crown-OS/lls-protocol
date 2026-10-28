use core::time::Duration;

use super::NetworkUsage;
use crate::clock::Instant;
use crate::units::Bitrate;

/// Turns the overuse verdict into a target bitrate.
///
/// Multiplicative increase, multiplicative decrease against the rate actually being delivered.
/// Decreasing against measured throughput rather than against the previous target matters: after
/// a Wi-Fi rate drop the previous target is fiction, and backing off from fiction overshoots.
#[derive(Debug)]
pub struct RateController {
    target: Bitrate,
    floor: Bitrate,
    ceiling: Bitrate,
    last_change: Option<Instant>,
}

impl RateController {
    /// Fast enough to reclaim a recovered link within a few frames, slow enough not to re-queue.
    const INCREASE_NUMERATOR: u32 = 21;
    const INCREASE_DENOMINATOR: u32 = 20;

    /// Backing off to 85 % of what the path actually delivered is the ratio GCC settled on, and
    /// it holds here: enough to drain a queue in a frame or two without collapsing quality.
    const DECREASE_NUMERATOR: u32 = 17;
    const DECREASE_DENOMINATOR: u32 = 20;

    /// One frame period at 60 Hz. Reacting faster than the encoder can respond only oscillates.
    const SETTLE: Duration = Duration::from_micros(16_667);

    pub const fn new(initial: Bitrate, floor: Bitrate, ceiling: Bitrate) -> Self {
        Self {
            target: initial,
            floor,
            ceiling,
            last_change: None,
        }
    }

    pub const fn target(&self) -> Bitrate {
        self.target
    }

    pub const fn set_bounds(&mut self, floor: Bitrate, ceiling: Bitrate) {
        self.floor = floor;
        self.ceiling = ceiling;
    }

    pub fn observe(&mut self, usage: NetworkUsage, delivered: Bitrate, now: Instant) -> Bitrate {
        if self
            .last_change
            .is_some_and(|last| now.saturating_elapsed_since(last) < Self::SETTLE)
        {
            return self.target;
        }
        self.last_change = Some(now);

        self.target = match usage {
            NetworkUsage::Overusing => {
                delivered.scaled_by(Self::DECREASE_NUMERATOR, Self::DECREASE_DENOMINATOR)
            }
            NetworkUsage::Underusing => self.target,
            NetworkUsage::Normal => self
                .target
                .scaled_by(Self::INCREASE_NUMERATOR, Self::INCREASE_DENOMINATOR),
        };
        self.clamp();
        self.target
    }

    fn clamp(&mut self) {
        if self.target < self.floor {
            self.target = self.floor;
        } else if self.target > self.ceiling {
            self.target = self.ceiling;
        }
    }
}
