#[derive()]
pub enum PacketType {
    Audio = 0,
    Video = 1,
}

pub enum FrameRateMode {
    Low = 30,
    Standard = 60,
    Smooth = 90,
    SuperSmooth = 120,
}

pub struct ConfigurationPacket {
    packet_type: PacketType,
    mode: FrameRateMode,
}

pub struct MediaPacket {
    pub packet_type: PacketType,

    /*
     * Sequence number is used to spot the missing packets
     */
    pub sequence_number: u16,

    /*
     * Sampling instant of the first octet in the media packet
     * Audio Sample Rate: 8KHz
     * Video Sample Rate: 90KHz
     * Example:
     *    - At 60fps  : increases 3000 per frame
     *    - At 120fps : increases 1500 per frame
     */
    pub timestamp: u32,

    /*
     * For video: marks the last packet of a video frame
     * to tell the decoder to render the frame
     */
    pub marker: bool,

    /*
     * 32-bit identifier that uniquely identifies
     * the origin of a media stream
     */
    pub ssrc: u32,
}

impl MediaPacket {
    fn parse() {}
}
