use bitflags::bitflags;

bitflags! {
    /// Per-packet hints the receiver acts on without parsing the payload.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct PacketFlags: u8 {
        /// Last fragment of a frame; the assembler may hand the frame to the decoder.
        const FRAME_END = 1 << 0;
        /// The frame this fragment belongs to decodes without any earlier frame.
        const KEYFRAME = 1 << 1;
        /// Last fragment of an independently decodable slice, so slice-level submission is safe.
        const SLICE_END = 1 << 2;
        /// Padding the congestion controller injected to keep the delay gradient observable.
        /// Never delivered to a decoder and never worth retransmitting.
        const DISCARDABLE = 1 << 3;
        /// Sent in response to a NACK rather than as part of the original burst.
        const RETRANSMISSION = 1 << 4;
        /// Sent to an address the session has not used yet, to prove the peer is reachable there
        /// before traffic moves to it.
        const PATH_CHALLENGE = 1 << 5;
        /// Parity of the key epoch that sealed the packet, so a receiver follows a rekey without
        /// a round trip.
        const KEY_PHASE = 1 << 6;
        /// A reliable message the receiver may deliver without waiting for earlier ones.
        const UNORDERED = 1 << 7;
    }
}
