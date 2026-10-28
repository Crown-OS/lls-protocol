use core::time::Duration;

use super::RepairRate;

/// When the receiver should ask for a lost fragment, and when it should stop asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetransmissionWindow {
    /// How long a gap must persist before it counts as loss rather than reordering.
    pub gap_settle: Duration,
    /// A request sent later than this before the playout deadline cannot come back in time, so
    /// the scheduler stops asking and lets the frame go.
    pub abandon_before_deadline: Duration,
}

/// What the engine will do about loss for one repair block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepairPlan {
    pub rate: RepairRate,
    pub parity_shards: u16,
    /// `None` when the round trip leaves no room for a request and a reply.
    pub retransmission: Option<RetransmissionWindow>,
}

impl RepairPlan {
    pub const fn sends_parity(&self) -> bool {
        self.parity_shards > 0
    }

    pub const fn requests_retransmission(&self) -> bool {
        self.retransmission.is_some()
    }
}
