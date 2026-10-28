use super::RepairRate;

/// How much parity the application wants, if any.
///
/// The knob the library exposes. [`RepairMode::Adaptive`] is the default because a fixed rate is
/// wrong twice over: it wastes bandwidth on a clean link and runs out of protection on a bad one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairMode {
    /// Never send parity. Loss is handled by retransmission alone, which is the right answer on
    /// USB and on a quiet wired LAN.
    Off,
    /// Always send this proportion, whatever the link is doing.
    Fixed(RepairRate),
    /// Let the policy choose within these bounds from the measured loss rate.
    Adaptive { min: RepairRate, max: RepairRate },
}

impl Default for RepairMode {
    fn default() -> Self {
        Self::Adaptive {
            min: RepairRate::NONE,
            max: RepairRate::MAX,
        }
    }
}
