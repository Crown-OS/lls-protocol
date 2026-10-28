use std::os::fd::AsRawFd;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError};
use std::time::Duration;

use llts_core::stream::StreamKey;
use llts_node::{Node, NodeInput};
use llts_transport::pairing::FileTrustStore;
use llts_transport::session::Driver;
use llts_wire::StreamClass;

use super::SharedAdvertisements;
use crate::command::Command;
use crate::error::describe;
use crate::event_json;
use crate::media::MediaPipelines;
use crate::store::{FfspLinks, PeerFeatureStore};
use crate::util::clock::NodeClock;
use crate::util::readiness::wait_readable;
use crate::util::wake::{WakeReceiver, Waker};

/// What the runtime thread owns: every piece of protocol state, touched by no other thread.
#[derive(Debug)]
pub(super) struct EventLoop {
    pub(super) driver: Driver<FileTrustStore>,
    pub(super) node: Node,
    pub(super) media: MediaPipelines,
    pub(super) clock: NodeClock,
    pub(super) features: PeerFeatureStore,
    pub(super) ffsp: FfspLinks,
    pub(super) port: u16,
    commands: Receiver<Command>,
    wake: WakeReceiver,
    events: SyncSender<String>,
    pub(super) advertisements: SharedAdvertisements,
}

/// The runtime thread's ends of the channels to and from the JNI side.
#[derive(Debug)]
pub(super) struct Wiring {
    pub(super) commands: Receiver<Command>,
    pub(super) wake: WakeReceiver,
    pub(super) waker: Waker,
    pub(super) events: SyncSender<String>,
    pub(super) advertisements: SharedAdvertisements,
}

/// Remote input of every feature travels on the session's first input stream, which both ends
/// open as soon as the session connects, so either side may send first.
pub(super) const INPUT_STREAM: StreamKey = StreamKey::new(StreamClass::Input, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Flow {
    Continue,
    Stop,
}

impl EventLoop {
    pub(super) fn new(
        driver: Driver<FileTrustStore>,
        node: Node,
        clock: NodeClock,
        features: PeerFeatureStore,
        ffsp: FfspLinks,
        port: u16,
        wiring: Wiring,
    ) -> Self {
        Self {
            driver,
            node,
            media: MediaPipelines::new(wiring.waker),
            clock,
            features,
            ffsp,
            port,
            commands: wiring.commands,
            wake: wiring.wake,
            events: wiring.events,
            advertisements: wiring.advertisements,
        }
    }

    pub(super) fn run(mut self) {
        self.drain_node();
        loop {
            let descriptors = [self.driver.as_raw_fd(), self.wake.as_raw_fd()];
            if let Err(error) = wait_readable(descriptors, self.next_wake()) {
                self.report(&error);
            }
            self.wake.drain();
            if self.execute_commands() == Flow::Stop {
                return;
            }
            self.pump_media();
            if let Err(error) = self.driver.poll() {
                self.report(&error);
            }
            self.route_driver_events();
            self.run_node_timers();
            self.drain_node();
        }
    }

    /// The sooner of the driver's and the node's deadlines; `None` sleeps until woken.
    fn next_wake(&self) -> Option<Duration> {
        let now = self.clock.now();
        let node = self
            .node
            .next_timeout()
            .map(|at| at.saturating_elapsed_since(now.instant));
        match (self.driver.next_timeout(), node) {
            (Some(driver), Some(node)) => Some(driver.min(node)),
            (driver, node) => driver.or(node),
        }
    }

    fn execute_commands(&mut self) -> Flow {
        loop {
            match self.commands.try_recv() {
                Ok(command) => {
                    if self.execute(command) == Flow::Stop {
                        return Flow::Stop;
                    }
                }
                Err(TryRecvError::Empty) => return Flow::Continue,
                Err(TryRecvError::Disconnected) => return Flow::Stop,
            }
        }
    }

    fn run_node_timers(&mut self) {
        let now = self.clock.now();
        if self
            .node
            .next_timeout()
            .is_some_and(|at| !at.is_after(now.instant))
        {
            self.node.handle(now, NodeInput::TimerExpired);
        }
    }

    pub(super) fn emit(&self, event: String) {
        if let Err(TrySendError::Full(_)) = self.events.try_send(event) {
            log::warn!("dropped an event: nothing is reading them");
        }
    }

    pub(super) fn report(&self, error: &dyn std::error::Error) {
        log::warn!("{}", describe(error));
        self.emit(event_json::error(error));
    }
}
