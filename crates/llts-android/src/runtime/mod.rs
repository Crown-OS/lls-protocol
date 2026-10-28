//! The runtime thread and the handle the JNI side holds to it.
//!
//! One thread owns the llts driver, the node and the media pipelines, and sleeps in `ppoll` on
//! the socket and a wake-up descriptor. Commands arrive over a channel, events leave over
//! another that Kotlin drains from its own thread with `nativeNextEvent`, so nothing here ever
//! calls into the JVM.

mod advertisements;
mod commands;
mod driver_events;
mod event_loop;
mod outputs;

use std::fs;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use llts_node::bridge::driver_class;
use llts_node::{AppEvent, KnownPeer, Node, NodeConfig};
use llts_signaling::device::DeviceId;
use llts_transport::pairing::{FileTrustStore, TrustStore};
use llts_transport::session::{Driver, DriverConfig};

pub(crate) use advertisements::SharedAdvertisements;
use event_loop::{EventLoop, Wiring};

use crate::command::Command;
use crate::config::StartConfig;
use crate::error::BridgeError;
use crate::event_json;
use crate::store::{FfspLinks, PeerFeatureStore};
use crate::util::clock::NodeClock;
use crate::util::wake::{Waker, wake_pair};

/// Events waiting for Kotlin beyond this many are dropped rather than letting a stalled reader
/// grow the queue without bound.
const EVENT_BACKLOG: usize = 1_024;

/// Everything the Java side holds behind one opaque handle.
#[derive(Debug)]
pub(crate) struct Instance {
    commands: Sender<Command>,
    waker: Waker,
    events: Mutex<Receiver<String>>,
    advertisements: SharedAdvertisements,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Instance {
    /// Opens the identity and trust store under `files_dir`, binds the llts socket and starts
    /// the runtime thread. Everything that can fail does so here, on the caller's thread.
    pub(crate) fn start(config: &StartConfig, files_dir: &Path) -> Result<Self, BridgeError> {
        let directory = files_dir.join("llts");
        fs::create_dir_all(&directory)?;
        let mut trust = FileTrustStore::open(directory.join("trust.bin"))?;
        let identity = trust.identity_or_generate()?;
        let features = PeerFeatureStore::open(directory.join("peer_features.bin"))?;
        let ffsp = FfspLinks::new(&files_dir.join("ffsp"));
        ffsp.adopt_identity(&identity)?;
        let node_config = NodeConfig::default();
        let known: Vec<KnownPeer> = trust
            .peers()
            .iter()
            .map(|peer| {
                let enabled = features
                    .enabled(&DeviceId(peer.static_key.0))
                    .unwrap_or(node_config.default_features);
                KnownPeer::from_trusted(peer, enabled)
            })
            .collect();

        let driver = Driver::bind(
            config.bind_address(),
            identity.clone(),
            trust,
            DriverConfig::new(config.name.as_str(), driver_class(config.class)),
        )?;
        let port = driver.local_addr()?.port();
        let clock = NodeClock::start();
        let node = Node::new(
            &identity,
            config.local_device(),
            known,
            node_config,
            clock.now(),
        )?;

        let (waker, wake) = wake_pair()?;
        let (commands, command_queue) = mpsc::channel();
        let (events, event_queue) = mpsc::sync_channel(EVENT_BACKLOG);
        let _ = events.try_send(event_json::ready(&node.id(), port));
        let _ = events.try_send(event_json::app_event(&AppEvent::DevicesChanged, &node));
        let advertisements = SharedAdvertisements::default();
        let wiring = Wiring {
            commands: command_queue,
            wake,
            waker: waker.clone(),
            events,
            advertisements: advertisements.clone(),
        };
        let thread = thread::Builder::new()
            .name("llts".to_owned())
            .spawn(move || {
                EventLoop::new(driver, node, clock, features, ffsp, port, wiring).run()
            })?;

        Ok(Self {
            commands,
            waker,
            events: Mutex::new(event_queue),
            advertisements,
            thread: Mutex::new(Some(thread)),
        })
    }

    pub(crate) fn send(&self, command: Command) -> Result<(), BridgeError> {
        self.commands
            .send(command)
            .map_err(|_| BridgeError::NotRunning)?;
        self.waker.wake();
        Ok(())
    }

    /// Blocks until the runtime has something to say; `None` once it has stopped.
    pub(crate) fn next_event(&self) -> Option<String> {
        let events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        events.recv().ok()
    }

    pub(crate) fn advertisements(&self) -> Vec<u8> {
        self.advertisements.encoded()
    }

    /// Stops the runtime thread and waits for it; a blocked `next_event` then returns `None`.
    pub(crate) fn stop(&self) {
        let _ = self.send(Command::Stop);
        let thread = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests;
