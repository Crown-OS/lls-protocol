//! A three-device mesh simulated in memory: every device owns its documents, subscribes to every
//! peer, and has to end up holding exactly what each owner last published.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test that fails a precondition should fail loudly"
)]

use std::collections::{BTreeMap, VecDeque};

use llts_signaling::device::{DeviceClass, DeviceId};
use llts_signaling::message::{Signal, Subscribe};
use llts_signaling::state::{
    Battery, Clipboard, ClipboardContent, DeviceInfo, Hotspot, HybridClock, Incarnation,
    Notification, Notifications, Published, StateCache, StateOutbox, StatePublisher, Topic,
    TopicPayload, TopicSet, Volume,
};

struct Datagram {
    from: DeviceId,
    to: DeviceId,
    bytes: Vec<u8>,
}

impl Datagram {
    fn new(from: DeviceId, to: DeviceId, signal: &Signal<'_>) -> Self {
        Self {
            from,
            to,
            bytes: signal.encode(Vec::new()).unwrap(),
        }
    }
}

struct Node {
    id: DeviceId,
    wall_ms: u64,
    clock: HybridClock,
    publisher: StatePublisher,
    cache: StateCache,
    outboxes: BTreeMap<DeviceId, StateOutbox>,
}

impl Node {
    fn new(seed: u8) -> Self {
        Self {
            id: DeviceId([seed; 32]),
            wall_ms: 1_000_000,
            clock: HybridClock::default(),
            publisher: StatePublisher::new(Incarnation::from_raw(u64::from(seed))),
            cache: StateCache::default(),
            outboxes: BTreeMap::new(),
        }
    }

    fn publish<'a, P: TopicPayload<'a>>(&mut self, value: &P) {
        if let Published::Changed(_) = self.publisher.publish(value).unwrap() {
            for outbox in self.outboxes.values_mut() {
                outbox.mark_changed(P::TOPIC);
            }
        }
    }

    fn copy(&mut self, text: &str) {
        let clipboard = Clipboard {
            mime: "text/plain;charset=utf-8",
            content: ClipboardContent::inline(text.as_bytes()).unwrap(),
            stamp: self.clock.tick(self.wall_ms),
        };
        self.publish(&clipboard);
    }

    fn receive(&mut self, from: DeviceId, bytes: &[u8], wire: &mut VecDeque<Datagram>) {
        match Signal::decode(bytes).unwrap() {
            Signal::SubscribeV1(subscribe) => {
                let snapshot = self
                    .outboxes
                    .entry(from)
                    .or_default()
                    .subscribe(subscribe.topics, &self.publisher);
                wire.push_back(Datagram::new(self.id, from, &Signal::SnapshotV1(snapshot)));
            }
            Signal::SnapshotV1(snapshot) => {
                self.cache.apply_snapshot(from, &snapshot);
                self.observe_clipboard_of(from);
            }
            Signal::UpdateV1(update) => {
                let _ = self.cache.apply_update(from, &update);
                self.observe_clipboard_of(from);
            }
            other => unreachable!("the mesh only exchanges state, got {other:?}"),
        }
    }

    fn observe_clipboard_of(&mut self, owner: DeviceId) {
        if let Some(clipboard) = self.cache.get::<Clipboard<'_>>(&owner).unwrap() {
            self.clock.observe(clipboard.stamp, self.wall_ms);
        }
    }

    fn newest_clipboard(&self) -> Option<(DeviceId, Vec<u8>)> {
        let own = self
            .publisher
            .get::<Clipboard<'_>>()
            .unwrap()
            .map(|clipboard| (self.id, clipboard));
        own.into_iter()
            .chain(self.cache.newest_clipboard())
            .max_by_key(|(origin, clipboard)| clipboard.order(*origin))
            .map(|(origin, clipboard)| match clipboard.content {
                ClipboardContent::Inline(bytes) => (origin, bytes.to_vec()),
                ClipboardContent::Offer { .. } => unreachable!("the mesh only copies text"),
            })
    }
}

struct Mesh {
    nodes: Vec<Node>,
    wire: VecDeque<Datagram>,
    updates_sent: usize,
}

impl Mesh {
    fn new(size: u8) -> Self {
        Self {
            nodes: (1..=size).map(Node::new).collect(),
            wire: VecDeque::new(),
            updates_sent: 0,
        }
    }

    fn id(&self, index: usize) -> DeviceId {
        self.nodes[index].id
    }

    fn connect(&mut self, subscriber: usize, owner: usize) {
        let (from, to) = (self.id(subscriber), self.id(owner));
        self.wire.push_back(Datagram::new(
            from,
            to,
            &Signal::SubscribeV1(Subscribe {
                topics: TopicSet::ALL,
            }),
        ));
    }

    fn connect_all(&mut self) {
        for subscriber in 0..self.nodes.len() {
            for owner in 0..self.nodes.len() {
                if subscriber != owner {
                    self.connect(subscriber, owner);
                }
            }
        }
    }

    fn flush(&mut self) {
        for node in &mut self.nodes {
            for (peer, outbox) in &mut node.outboxes {
                for update in outbox.drain(&node.publisher) {
                    self.wire
                        .push_back(Datagram::new(node.id, *peer, &Signal::UpdateV1(update)));
                    self.updates_sent += 1;
                }
            }
        }
    }

    fn settle(&mut self) {
        loop {
            self.flush();
            let Some(datagram) = self.wire.pop_front() else {
                return;
            };
            let node = self
                .nodes
                .iter_mut()
                .find(|node| node.id == datagram.to)
                .unwrap();
            node.receive(datagram.from, &datagram.bytes, &mut self.wire);
        }
    }

    /// The process dies and comes back: its sessions, cache and publisher are all gone, and it
    /// draws a fresh incarnation.
    fn restart(&mut self, index: usize, incarnation: u64) {
        let id = self.id(index);
        self.wire
            .retain(|datagram| datagram.from != id && datagram.to != id);
        for node in &mut self.nodes {
            node.outboxes.remove(&id);
        }
        let node = &mut self.nodes[index];
        node.publisher = StatePublisher::new(Incarnation::from_raw(incarnation));
        node.cache = StateCache::default();
        node.outboxes.clear();
        node.clock = HybridClock::default();
    }

    fn reconnect(&mut self, index: usize) {
        for other in 0..self.nodes.len() {
            if other != index {
                self.connect(index, other);
                self.connect(other, index);
            }
        }
    }

    fn assert_converged(&self) {
        for viewer in &self.nodes {
            for owner in self.nodes.iter().filter(|owner| owner.id != viewer.id) {
                for topic in Topic::ALL {
                    assert_eq!(
                        viewer.cache.update(&owner.id, topic),
                        owner.publisher.update(topic),
                        "{} sees a different {topic:?} for {}",
                        viewer.id,
                        owner.id
                    );
                }
            }
        }
        let clipboards: Vec<_> = self.nodes.iter().map(Node::newest_clipboard).collect();
        assert!(
            clipboards.windows(2).all(|pair| pair[0] == pair[1]),
            "devices disagree on the clipboard: {clipboards:?}"
        );
    }
}

const fn battery(percent: u8) -> Battery {
    Battery {
        percent,
        charging: percent < 20,
        time_to_empty_min: None,
    }
}

fn introduce(node: &mut Node, name: &str, class: DeviceClass) {
    node.publish(&DeviceInfo {
        name,
        model: "sim",
        class,
    });
}

#[test]
fn three_devices_converge_on_each_others_state() {
    let mut mesh = Mesh::new(3);
    introduce(&mut mesh.nodes[0], "laptop", DeviceClass::Computer);
    introduce(&mut mesh.nodes[1], "phone", DeviceClass::Phone);
    introduce(&mut mesh.nodes[2], "tablet", DeviceClass::Tablet);
    mesh.nodes[1].publish(&battery(88));
    mesh.nodes[2].publish(&battery(41));

    mesh.connect_all();
    mesh.settle();
    mesh.assert_converged();
    assert_eq!(
        mesh.updates_sent, 0,
        "a new session is served by snapshots alone"
    );

    for percent in [87, 86, 85] {
        mesh.nodes[1].publish(&battery(percent));
    }
    mesh.nodes[0].publish(&Volume {
        percent: 30,
        muted: false,
    });
    mesh.nodes[2].publish(&Notifications {
        active: vec![Notification {
            id: "0|com.chat|12",
            app: "Chat",
            title: "Ada",
            body: "lunch?",
            actions: Vec::new(),
        }],
    });
    mesh.settle();
    mesh.assert_converged();
    assert_eq!(
        mesh.updates_sent, 6,
        "three battery changes coalesce to one update per subscriber"
    );

    let phone = mesh.id(1);
    assert_eq!(
        mesh.nodes[2].cache.get::<Battery>(&phone).unwrap(),
        Some(battery(85))
    );
}

#[test]
fn the_mesh_reconverges_after_an_owner_restarts() {
    let mut mesh = Mesh::new(3);
    for percent in (60..=90).rev() {
        mesh.nodes[1].publish(&battery(percent));
    }
    mesh.nodes[1].publish(&Hotspot {
        enabled: true,
        ssid: Some("phone-ap"),
    });
    mesh.nodes[0].publish(&battery(100));
    mesh.connect_all();
    mesh.settle();
    mesh.assert_converged();

    let phone = mesh.id(1);
    let stale_seq = mesh.nodes[0]
        .cache
        .version(&phone, Topic::Battery)
        .unwrap()
        .seq;

    mesh.restart(1, 0xBEEF);
    mesh.nodes[1].publish(&battery(15));
    introduce(&mut mesh.nodes[1], "phone", DeviceClass::Phone);
    assert!(
        mesh.nodes[1]
            .publisher
            .update(Topic::Battery)
            .unwrap()
            .version
            .seq
            < stale_seq
    );

    mesh.reconnect(1);
    mesh.settle();
    mesh.assert_converged();

    for viewer in [0, 2] {
        let cache = &mesh.nodes[viewer].cache;
        assert_eq!(cache.get::<Battery>(&phone).unwrap(), Some(battery(15)));
        assert_eq!(cache.get::<Hotspot<'_>>(&phone).unwrap(), None);
        assert_eq!(
            cache.version(&phone, Topic::Battery).unwrap().incarnation,
            Incarnation::from_raw(0xBEEF)
        );
    }

    mesh.nodes[1].publish(&battery(14));
    mesh.settle();
    mesh.assert_converged();
}

#[test]
fn a_restarted_device_relearns_its_peers() {
    let mut mesh = Mesh::new(3);
    mesh.nodes[0].publish(&battery(50));
    mesh.nodes[2].publish(&battery(20));
    mesh.connect_all();
    mesh.settle();

    mesh.restart(0, 0xCAFE);
    assert_eq!(mesh.nodes[0].cache.owners().count(), 0);
    mesh.reconnect(0);
    mesh.settle();
    mesh.assert_converged();
}

#[test]
fn the_clipboard_converges_on_the_latest_copy_even_from_a_clock_that_runs_behind() {
    let mut mesh = Mesh::new(3);
    mesh.nodes[1].wall_ms = 400_000;
    mesh.connect_all();
    mesh.settle();

    mesh.nodes[0].copy("from the laptop");
    mesh.settle();
    mesh.assert_converged();
    assert_eq!(
        mesh.nodes[2].newest_clipboard(),
        Some((mesh.id(0), b"from the laptop".to_vec()))
    );

    mesh.nodes[1].copy("from the phone, whose clock is ten minutes slow");
    mesh.settle();
    mesh.assert_converged();
    assert_eq!(
        mesh.nodes[0].newest_clipboard().map(|(origin, _)| origin),
        Some(mesh.id(1)),
        "a later copy must win even though its wall clock reads earlier"
    );

    mesh.nodes[2].copy("from the tablet");
    mesh.settle();
    mesh.assert_converged();
    assert_eq!(
        mesh.nodes[1].newest_clipboard().map(|(origin, _)| origin),
        Some(mesh.id(2))
    );
}
