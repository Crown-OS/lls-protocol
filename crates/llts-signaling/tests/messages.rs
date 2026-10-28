#![allow(
    clippy::unwrap_used,
    reason = "a test that fails a precondition should fail loudly"
)]

use std::collections::HashSet;

use llts_signaling::SignalingError;
use llts_signaling::device::{DeviceClass, Feature, FeatureSet};
use llts_signaling::message::{
    AcceptStream, AudioCodec, AudioParams, Call, CameraLens, Capabilities, CloseStream, CodecList,
    Direction, Edge, EdgePosition, FIRST_APPLICATION_TYPE, FfspTransferId, FileOffer, Goodbye,
    GoodbyeReason, KeyframeRequest, Lock, Media, Message, MessageType, MuxClass, OpenStream, Rekey,
    Reply, SetHotspot, SetVolume, Signal, Snapshot, StartCamera, StartMic, StartMirror,
    StartMonitor, Stop, StreamCodec, StreamParams, StreamRef, Subscribe, UnicursorEnter,
    UnicursorLeave, Unsubscribe, Update, VideoCodec, VideoLimits, VideoParams,
};
use llts_signaling::state::{Incarnation, Topic, TopicSet, Version};

const VIDEO: StreamRef = StreamRef {
    class: MuxClass::Video,
    index: 0,
};

const LIMITS: VideoLimits = VideoLimits {
    max_width: 2560,
    max_height: 1600,
    max_fps: 120,
};

const VERSION: Version = Version {
    incarnation: Incarnation::from_raw(0xDEAD_BEEF_F00D),
    seq: 42,
};

fn capabilities() -> Capabilities {
    Capabilities {
        video_codecs: CodecList::from_slice(&[VideoCodec::Hevc, VideoCodec::H264]).unwrap(),
        audio_codecs: CodecList::from_slice(&[AudioCodec::Opus]).unwrap(),
        features: [Feature::Mirror, Feature::Clipboard, Feature::Files]
            .into_iter()
            .collect(),
        device_class: DeviceClass::Computer,
    }
}

fn every_message() -> Vec<Signal<'static>> {
    let update = Update {
        topic: Topic::Battery,
        version: VERSION,
        payload: &[80, 1, 0],
    };
    vec![
        Signal::CapabilitiesV1(capabilities()),
        Signal::GoodbyeV1(Goodbye {
            reason: GoodbyeReason::Unpaired,
        }),
        Signal::RekeyV1(Rekey { epoch: 7 }),
        Signal::KeyframeRequestV1(KeyframeRequest { stream: VIDEO }),
        Signal::OpenStreamV1(OpenStream {
            index: 0,
            purpose: Feature::Mirror,
            params: StreamParams::Video(VideoParams {
                codec: VideoCodec::Hevc,
                width: 2560,
                height: 1600,
                fps_mhz: 59_940,
                bitrate_kbps: 40_000,
            }),
        }),
        Signal::OpenStreamV1(OpenStream {
            index: 1,
            purpose: Feature::Mic,
            params: StreamParams::Audio(AudioParams {
                codec: AudioCodec::Opus,
                sample_rate: 48_000,
                channels: 1,
                frame_ms: 10,
            }),
        }),
        Signal::AcceptStreamV1(AcceptStream {
            stream: VIDEO,
            codec: StreamCodec::Video(VideoCodec::H264),
        }),
        Signal::CloseStreamV1(CloseStream { stream: VIDEO }),
        Signal::SubscribeV1(Subscribe {
            topics: TopicSet::ALL,
        }),
        Signal::UnsubscribeV1(Unsubscribe {
            topics: Topic::Clipboard.into(),
        }),
        Signal::UpdateV1(update),
        Signal::SnapshotV1(Snapshot {
            topics: [Topic::Battery, Topic::Volume].into_iter().collect(),
            updates: vec![update],
        }),
        Signal::CallV1(Call::Pickup { call_id: "call-1" }),
        Signal::CallV1(Call::Decline { call_id: "call-2" }),
        Signal::CallV1(Call::Hangup { call_id: "call-3" }),
        Signal::CallV1(Call::Dial {
            number: "+44 20 7946 0000",
        }),
        Signal::MediaV1(Media::Play),
        Signal::MediaV1(Media::Seek { percent: 63 }),
        Signal::ReplyV1(Reply {
            conversation_id: "thread/9",
            text: "on my way ✓",
        }),
        Signal::SetVolumeV1(SetVolume {
            percent: 35,
            muted: false,
        }),
        Signal::LockV1(Lock),
        Signal::SetHotspotV1(SetHotspot { enabled: true }),
        Signal::FileOfferV1(FileOffer {
            ffsp_transfer_id: FfspTransferId(0xFFFF_FFFF),
        }),
        Signal::StartMirrorV1(StartMirror {
            direction: Direction::FromPeer,
            limits: LIMITS,
            remote_input: true,
        }),
        Signal::StartCameraV1(StartCamera {
            lens: CameraLens::Back,
            limits: LIMITS,
        }),
        Signal::StartMicV1(StartMic),
        Signal::StartMonitorV1(StartMonitor {
            width: 2000,
            height: 1200,
            refresh_mhz: 60_000,
            scale_120: 180,
            placement: Edge::Right,
        }),
        Signal::UnicursorEnterV1(UnicursorEnter {
            edge: Edge::Left,
            position: EdgePosition(0x8000),
        }),
        Signal::UnicursorLeaveV1(UnicursorLeave {
            edge: Edge::Top,
            position: EdgePosition(u16::MAX),
        }),
        Signal::StopV1(Stop {
            feature: Feature::Monitor,
        }),
    ]
}

#[test]
fn every_registered_message_round_trips() {
    let messages = every_message();
    for message in &messages {
        let encoded = message.encode(Vec::new()).unwrap();
        assert_eq!(&Signal::decode(&encoded).unwrap(), message);
    }

    let covered: HashSet<MessageType> = messages.iter().filter_map(Signal::message_type).collect();
    let registered: HashSet<MessageType> = MessageType::ALL.iter().copied().collect();
    assert_eq!(
        covered, registered,
        "a registered message has no round-trip sample"
    );
}

#[test]
fn the_envelope_is_the_type_code_then_the_postcard_body() {
    let encoded = Rekey { epoch: 300 }.encode(Vec::new()).unwrap();
    assert_eq!(encoded, [0x02, 0x01, 0xAC, 0x02]);
    assert_eq!(MessageType::RekeyV1.as_u16(), 0x0102);
}

#[test]
fn encoding_appends_to_the_buffer_it_is_given() {
    let first = Lock.encode(Vec::new()).unwrap();
    let both = StartMic.encode(first.clone()).unwrap();
    assert_eq!(both.get(..first.len()), Some(first.as_slice()));
    assert_eq!(both.len(), 4);
}

#[test]
fn every_type_code_stays_clear_of_the_block_the_transport_reserves() {
    for &ty in MessageType::ALL {
        assert!(
            ty.as_u16() >= FIRST_APPLICATION_TYPE,
            "{ty:?} uses {:#06x}, which llts-core keeps for itself",
            ty.as_u16()
        );
    }
    assert_eq!(FIRST_APPLICATION_TYPE, 0x0100);
}

#[test]
fn type_codes_are_unique_and_zero_is_reserved() {
    let codes: HashSet<u16> = MessageType::ALL.iter().map(|ty| ty.as_u16()).collect();
    assert_eq!(codes.len(), MessageType::ALL.len());
    assert!(!codes.contains(&0));
    for &ty in MessageType::ALL {
        assert_eq!(MessageType::from_u16(ty.as_u16()), Some(ty));
    }
}

#[test]
fn an_unknown_message_type_is_skipped_rather_than_failing_the_stream() {
    let mut from_newer_peer = 0x7F01_u16.to_le_bytes().to_vec();
    from_newer_peer.extend_from_slice(b"a body this build cannot read");
    let stream = [
        Lock.encode(Vec::new()).unwrap(),
        from_newer_peer,
        Rekey { epoch: 1 }.encode(Vec::new()).unwrap(),
    ];

    let decoded: Vec<Signal<'_>> = stream
        .iter()
        .map(|message| Signal::decode(message).unwrap())
        .collect();
    assert_eq!(
        decoded,
        [
            Signal::LockV1(Lock),
            Signal::Unknown { msg_type: 0x7F01 },
            Signal::RekeyV1(Rekey { epoch: 1 }),
        ]
    );

    let known: Vec<_> = decoded
        .iter()
        .filter(|signal| signal.message_type().is_some())
        .collect();
    assert_eq!(known.len(), 2);
    assert_eq!(
        Signal::Unknown { msg_type: 0x7F01 }.encode(Vec::new()),
        Err(SignalingError::UnknownMessageType(0x7F01))
    );
}

#[test]
fn a_message_too_short_for_its_type_is_rejected() {
    for short in [&[][..], &[0x01]] {
        assert_eq!(
            Signal::decode(short),
            Err(SignalingError::TruncatedEnvelope { len: short.len() })
        );
    }
}

#[test]
fn a_known_type_with_a_malformed_body_is_an_error_not_a_skip() {
    let mut encoded = Reply {
        conversation_id: "c",
        text: "hello",
    }
    .encode(Vec::new())
    .unwrap();
    encoded.truncate(encoded.len() - 2);
    assert!(matches!(
        Signal::decode(&encoded),
        Err(SignalingError::MalformedBody {
            msg_type: MessageType::ReplyV1,
            ..
        })
    ));
}

#[test]
fn decoded_strings_borrow_from_the_received_buffer() {
    let encoded = Reply {
        conversation_id: "thread",
        text: "zero copy",
    }
    .encode(Vec::new())
    .unwrap();
    let Signal::ReplyV1(reply) = Signal::decode(&encoded).unwrap() else {
        unreachable!("encoded a reply");
    };
    let buffer = encoded.as_ptr_range();
    assert!(buffer.contains(&reply.text.as_ptr()));
}

#[test]
fn video_negotiation_keeps_the_choosers_preference_and_falls_back_to_h264() {
    let laptop = capabilities();
    let mut phone = capabilities();
    phone.video_codecs = CodecList::from_slice(&[VideoCodec::Av1, VideoCodec::H264]).unwrap();
    assert_eq!(laptop.video_codec_with(&phone), Some(VideoCodec::H264));
    assert_eq!(laptop.video_codec_with(&laptop), Some(VideoCodec::Hevc));
    assert_eq!(laptop.audio_codec_with(&phone), Some(AudioCodec::Opus));

    phone.video_codecs.clear();
    assert_eq!(laptop.video_codec_with(&phone), None);
}

#[test]
fn a_stream_takes_its_class_from_its_parameters() {
    let Some(Signal::OpenStreamV1(open)) = every_message().into_iter().nth(5) else {
        unreachable!("the sixth sample opens an audio stream");
    };
    assert_eq!(
        open.stream(),
        StreamRef {
            class: MuxClass::Audio,
            index: 1
        }
    );
    assert_eq!(open.params.codec(), StreamCodec::Audio(AudioCodec::Opus));
}

#[test]
fn feature_sets_keep_only_what_both_ends_share() {
    let ours: FeatureSet = [Feature::Mirror, Feature::Calls].into_iter().collect();
    let theirs: FeatureSet = [Feature::Calls, Feature::Hotspot].into_iter().collect();
    assert_eq!(
        ours.shared_with(theirs).iter().collect::<Vec<_>>(),
        [Feature::Calls]
    );
}
