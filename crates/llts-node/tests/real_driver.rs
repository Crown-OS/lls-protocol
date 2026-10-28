//! Two nodes on real llts drivers over localhost, joined only by `bridge`: a QR pairing, state
//! sync and a camera stream negotiated end to end through encrypted sessions.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test that fails a precondition should fail loudly"
)]

use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant as WallClock, SystemTime, UNIX_EPOCH};

use llts_core::clock::Instant;
use llts_node::bridge::{driver_class, execute, link_event};
use llts_node::{
    AppCommand, AppEvent, FeatureRequest, LinkEvent, LocalDevice, MediaRequest, MediaRole, Node,
    NodeConfig, NodeError, NodeInput, NodeOutput, Now,
};
use llts_signaling::device::{DeviceClass, Feature};
use llts_signaling::message::{
    AudioCodec, CameraLens, Capabilities, CodecList, StartCamera, VideoCodec, VideoLimits,
};
use llts_signaling::state::Battery;
use llts_transport::pairing::{MemoryTrustStore, TrustStore};
use llts_transport::security::DeviceIdentity;
use llts_transport::session::{Driver, DriverConfig};

struct Runtime {
    driver: Driver<MemoryTrustStore>,
    node: Node,
    origin: WallClock,
    events: Vec<AppEvent>,
    media: Vec<MediaRequest>,
    failures: Vec<NodeError>,
}

impl Runtime {
    fn new(name: &str, class: DeviceClass) -> Self {
        let identity = DeviceIdentity::generate().unwrap();
        let mut trust = MemoryTrustStore::new();
        trust.set_identity(&identity).unwrap();
        let local = LocalDevice {
            name: name.to_owned(),
            model: "loopback".to_owned(),
            capabilities: Capabilities {
                video_codecs: CodecList::from_slice(&[VideoCodec::Hevc, VideoCodec::H264]).unwrap(),
                audio_codecs: CodecList::from_slice(&[AudioCodec::Opus]).unwrap(),
                features: Feature::ALL.into_iter().collect(),
                device_class: class,
            },
        };
        let driver = Driver::bind(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            identity.clone(),
            trust,
            DriverConfig::new(name, driver_class(class)),
        )
        .unwrap();
        let origin = WallClock::now();
        let node = Node::new(&identity, local, [], NodeConfig::default(), now(origin)).unwrap();
        let mut runtime = Self {
            driver,
            node,
            origin,
            events: Vec::new(),
            media: Vec::new(),
            failures: Vec::new(),
        };
        let address = runtime.driver.local_addr().unwrap();
        runtime.handle(NodeInput::NetworkChanged {
            addresses: &[address],
            rebind: None,
        });
        runtime
    }

    fn handle(&mut self, input: NodeInput<'_>) {
        self.node.handle(now(self.origin), input);
        self.drain();
    }

    fn drain(&mut self) {
        while let Some(output) = self.node.poll_output() {
            match output {
                NodeOutput::Driver(command) => match execute(&mut self.driver, command) {
                    Ok(Some(offer)) => self.node.handle(
                        now(self.origin),
                        NodeInput::Link(LinkEvent::QrOfferReady { offer: &offer }),
                    ),
                    Ok(None) => {}
                    Err(error) => self.failures.push(error),
                },
                NodeOutput::App(event) => self.events.push(event),
                NodeOutput::Media(request) => self.media.push(request),
                NodeOutput::Persist(_)
                | NodeOutput::ExportFfspLink(_)
                | NodeOutput::Advertise(_) => {}
            }
        }
    }

    fn pump(&mut self) {
        self.driver.wait(Duration::from_millis(1)).unwrap();
        while let Some(event) = self.driver.poll_event() {
            if let Some(link) = link_event(&event) {
                self.node.handle(now(self.origin), NodeInput::Link(link));
            }
        }
        if self
            .node
            .next_timeout()
            .is_some_and(|at| !at.is_after(now(self.origin).instant))
        {
            self.node.handle(now(self.origin), NodeInput::TimerExpired);
        }
        self.drain();
    }
}

fn now(origin: WallClock) -> Now {
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        });
    let micros = u64::try_from(origin.elapsed().as_micros()).unwrap_or(u64::MAX);
    Now::new(Instant::from_micros(micros), unix_ms)
}

fn run_until(
    laptop: &mut Runtime,
    phone: &mut Runtime,
    done: impl Fn(&Runtime, &Runtime) -> bool,
) -> bool {
    let deadline = WallClock::now() + Duration::from_secs(5);
    while WallClock::now() < deadline {
        laptop.pump();
        phone.pump();
        if done(laptop, phone) {
            return true;
        }
    }
    false
}

#[test]
fn two_nodes_pair_sync_and_stream_through_real_drivers() {
    let mut laptop = Runtime::new("laptop", DeviceClass::Computer);
    let mut phone = Runtime::new("phone", DeviceClass::Phone);

    laptop.handle(NodeInput::App(AppCommand::BeginPairing { duration: None }));
    let uri = laptop
        .events
        .iter()
        .find_map(|event| match event {
            AppEvent::PairingOffer { uri, .. } => Some(uri.clone()),
            _ => None,
        })
        .expect("the laptop shows a QR offer");
    phone.handle(NodeInput::App(AppCommand::PairWithQr { uri: &uri }));

    let (laptop_id, phone_id) = (laptop.node.id(), phone.node.id());
    let battery = Battery {
        percent: 58,
        charging: true,
        time_to_empty_min: None,
    };
    assert!(
        run_until(&mut laptop, &mut phone, |laptop, phone| {
            laptop
                .node
                .device(&phone_id)
                .is_some_and(|device| device.connected)
                && phone
                    .node
                    .device(&laptop_id)
                    .is_some_and(|device| device.connected)
        }),
        "the devices never connected"
    );
    let _ = phone.node.publish(now(phone.origin), &battery).unwrap();
    assert!(
        run_until(&mut laptop, &mut phone, |laptop, _| {
            laptop
                .node
                .remote_state::<Battery>(&phone_id)
                .ok()
                .flatten()
                == Some(battery)
        }),
        "the phone's battery never reached the laptop"
    );
    assert!(
        laptop
            .driver
            .trust()
            .peer(phone.driver.identity().public())
            .is_some()
    );

    laptop.handle(NodeInput::App(AppCommand::StartFeature {
        peer: phone_id,
        request: FeatureRequest::Camera(StartCamera {
            lens: CameraLens::Front,
            limits: VideoLimits {
                max_width: 1280,
                max_height: 720,
                max_fps: 30,
            },
        }),
    }));
    assert!(
        run_until(&mut laptop, &mut phone, |laptop, phone| {
            let started = |runtime: &Runtime, role| {
                runtime
                    .media
                    .iter()
                    .any(|request| matches!(request, MediaRequest::Start { role: held, .. } if *held == role))
            };
            started(laptop, MediaRole::Decoder) && started(phone, MediaRole::Encoder)
        }),
        "the camera stream was never negotiated"
    );
    assert!(laptop.failures.is_empty(), "{:?}", laptop.failures);
    assert!(phone.failures.is_empty(), "{:?}", phone.failures);
}
