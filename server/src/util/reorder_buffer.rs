use std::collections::BTreeMap;

use crate::packet::MediaPacket;

#[derive(Default)]
pub struct ReorderBuffer {
    // u16: Sequence number
    packets: BTreeMap<u16, MediaPacket>,
    next_sequence_number: Option<u16>,
}

impl ReorderBuffer {
    pub fn insert(&mut self, packet: MediaPacket) {
        if self.next_sequence_number.is_none() {
            self.next_sequence_number = Some(packet.sequence_number);
        }

        self.packets.insert(packet.sequence_number, packet);
    }
}
