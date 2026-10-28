use llts_wire::MonotonicMicros;

use super::AckedPacket;

/// Packets that left within this window are treated as one burst, so pacing jitter inside a
/// frame's burst is not mistaken for queueing.
const BURST_WINDOW_MICROS: u32 = 3_000;

/// One burst of packets, summarised.
#[derive(Debug, Clone, Copy)]
struct ArrivalGroup {
    first_departure: MonotonicMicros,
    last_departure: MonotonicMicros,
    last_arrival: MonotonicMicros,
    byte_len: u32,
}

/// How much longer a burst took to arrive than it took to leave.
///
/// Positive means the path is absorbing the difference into a queue, which is the signal a
/// delay-based controller acts on — and it appears well before any packet is actually lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelayDelta {
    pub micros: i32,
    pub bytes_between_groups: u32,
}

/// Groups acknowledged packets into bursts and reports the delay change between consecutive ones.
#[derive(Debug, Default)]
pub struct InterArrivalFilter {
    current: Option<ArrivalGroup>,
    previous: Option<ArrivalGroup>,
}

impl InterArrivalFilter {
    pub const fn new() -> Self {
        Self {
            current: None,
            previous: None,
        }
    }

    /// Folds in one acknowledgement, yielding a delta whenever a burst closes.
    pub fn accept(&mut self, packet: AckedPacket) -> Option<DelayDelta> {
        let Some(group) = self.current.as_mut() else {
            self.current = Some(ArrivalGroup {
                first_departure: packet.departure,
                last_departure: packet.departure,
                last_arrival: packet.arrival,
                byte_len: packet.byte_len,
            });
            return None;
        };

        if packet.departure.elapsed_since(group.first_departure) < BURST_WINDOW_MICROS {
            group.last_departure = packet.departure;
            group.last_arrival = packet.arrival;
            group.byte_len = group.byte_len.saturating_add(packet.byte_len);
            return None;
        }

        let closed = *group;
        self.current = Some(ArrivalGroup {
            first_departure: packet.departure,
            last_departure: packet.departure,
            last_arrival: packet.arrival,
            byte_len: packet.byte_len,
        });

        let delta = self.previous.map(|earlier| DelayDelta {
            micros: i64_to_i32(
                i64::from(closed.last_arrival.elapsed_since(earlier.last_arrival))
                    - i64::from(closed.last_departure.elapsed_since(earlier.last_departure)),
            ),
            bytes_between_groups: closed.byte_len,
        });
        self.previous = Some(closed);
        delta
    }
}

fn i64_to_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value.is_negative() {
        i32::MIN
    } else {
        i32::MAX
    })
}
