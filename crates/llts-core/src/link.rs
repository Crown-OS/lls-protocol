use core::time::Duration;

use crate::units::LossRate;

/// The physical path a session is currently running over.
///
/// The repair policy branches on this because the trade-off is not a matter of degree: a USB bulk
/// pipe has CRC and hardware retry underneath it, so parity bytes there buy nothing at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinkKind {
    Wifi,
    Ethernet,
    Usb,
    /// Both endpoints in one process, used by tests and the loopback bench.
    Loopback,
}

impl LinkKind {
    /// Whether the link loses packets often enough for parity to earn its bandwidth.
    pub const fn loses_packets(self) -> bool {
        matches!(self, Self::Wifi | Self::Ethernet)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Wifi => "wi-fi",
            Self::Ethernet => "ethernet",
            Self::Usb => "usb",
            Self::Loopback => "loopback",
        }
    }
}

/// What the receiver's feedback says the path is currently doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkQuality {
    pub kind: LinkKind,
    pub loss_rate: LossRate,
    /// Minimum-filtered round trip, which is the delay a retransmission has to fit inside.
    pub round_trip: Duration,
}

impl LinkQuality {
    pub const fn pristine(kind: LinkKind) -> Self {
        Self {
            kind,
            loss_rate: LossRate::NONE,
            round_trip: Duration::from_micros(0),
        }
    }
}
