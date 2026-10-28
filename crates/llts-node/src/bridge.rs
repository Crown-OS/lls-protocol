//! The two translations between a [`Node`](crate::Node) and a real llts [`Driver`].
//!
//! A runtime loop is then: poll the driver, pass each [`DriverEvent`] through [`link_event`] to
//! the node (or, for media, to its pipelines), and carry out each
//! [`NodeOutput::Driver`](crate::NodeOutput::Driver) with [`execute`].

use core::num::NonZeroU16;

use llts_core::session::{Action, Event};
use llts_core::stream::MediaStreamConfig;
use llts_core::units::FrameRate;
use llts_signaling::device::{DeviceClass, DeviceId};
use llts_signaling::message::{MuxClass, StreamParams};
use llts_transport::TransportError;
use llts_transport::pairing::{QrOffer, TrustStore};
use llts_transport::security::DeviceClass as TransportClass;
use llts_transport::session::{Driver, DriverEvent};

use crate::NodeError;
use crate::input::LinkEvent;
use crate::output::DriverCommand;
use crate::util::device::{class_from_transport, device_id, static_key};
use crate::util::stream::{stream_key, stream_ref};

/// What a driver event means to the node; `None` for session actions the runtime handles
/// itself: frames to present, input batches, encoder rate changes and clock updates.
pub fn link_event(event: &DriverEvent) -> Option<LinkEvent<'_>> {
    Some(match event {
        DriverEvent::Connected { peer, hello } => LinkEvent::Connected {
            peer: device_id(*peer),
            name: &hello.name,
            class: class_from_transport(hello.class),
        },
        DriverEvent::PairingCode { peer, hello, code } => LinkEvent::PairingCode {
            peer: device_id(*peer),
            name: &hello.name,
            code: code.value(),
        },
        DriverEvent::Paired { peer } => LinkEvent::Paired {
            peer: device_id(*peer),
        },
        DriverEvent::HandshakeFailed { address, .. } => {
            LinkEvent::HandshakeFailed { address: *address }
        }
        DriverEvent::Session {
            peer,
            action: Action::PeerDead,
        } => LinkEvent::Disconnected {
            peer: device_id(*peer),
        },
        DriverEvent::Session {
            peer,
            action: Action::Deliver { stream, message },
        } => {
            let stream = stream_ref(*stream)
                .filter(|stream| matches!(stream.class, MuxClass::Control | MuxClass::Data))?;
            LinkEvent::Message {
                peer: device_id(*peer),
                stream,
                bytes: message.filled(),
            }
        }
        DriverEvent::Session { .. } => return None,
    })
}

/// Carries out one driver command. A QR offer the driver produced comes back, to be handed to
/// the node as [`LinkEvent::QrOfferReady`].
///
/// # Errors
///
/// Returns what the driver refused, and [`NodeError::NotConnected`] for a stream command to a
/// peer whose session is gone.
pub fn execute<T: TrustStore>(
    driver: &mut Driver<T>,
    command: DriverCommand,
) -> Result<Option<QrOffer>, NodeError> {
    match command {
        DriverCommand::Dial { peer, address } => driver.connect(static_key(peer), address)?,
        DriverCommand::OfferQrPairing {
            addresses,
            duration,
        } => return Ok(Some(driver.offer_qr_pairing(addresses, duration)?)),
        DriverCommand::ClosePairing => driver.close_pairing(),
        DriverCommand::PairNearby { address } => driver.pair_nearby(address)?,
        DriverCommand::PairWithOffer { offer } => driver.pair_with_offer(&offer)?,
        DriverCommand::ConfirmPairing { peer } => driver.confirm_pairing(&static_key(peer))?,
        DriverCommand::RejectPairing { peer } => driver.reject_pairing(&static_key(peer)),
        DriverCommand::Send {
            peer,
            stream,
            message,
            delivery,
        } => {
            let stream = stream_key(stream).ok_or(NodeError::NotConnected(peer))?;
            driver.handle(
                &static_key(peer),
                &Event::ControlQueued {
                    stream,
                    message: &message,
                    delivery,
                },
            )?;
        }
        DriverCommand::OpenReliable { peer, stream } => {
            let key = stream_key(stream).ok_or(NodeError::NotConnected(peer))?;
            session(driver, peer)?
                .open_reliable(key)
                .map_err(TransportError::from)?;
        }
        DriverCommand::OpenMedia {
            peer,
            stream,
            params,
            role,
        } => {
            let key = stream_key(stream).ok_or(NodeError::NotConnected(peer))?;
            let config = match params {
                StreamParams::Video(video) => MediaStreamConfig {
                    frame_rate: frame_rate(video.fps_mhz),
                    ..MediaStreamConfig::screen_share(role)
                },
                StreamParams::Audio(_) => MediaStreamConfig::opus_voice(role),
            };
            session(driver, peer)?
                .open_media(key, config)
                .map_err(TransportError::from)?;
        }
        DriverCommand::CloseStream { peer, stream } => {
            if let (Some(key), Ok(session)) = (stream_key(stream), session(driver, peer)) {
                session.close(key);
            }
        }
        DriverCommand::Forget { peer } => {
            let _ = driver.forget(&static_key(peer))?;
        }
        DriverCommand::Rebind { local } => driver.rebind(local)?,
    }
    Ok(None)
}

fn session<T: TrustStore>(
    driver: &mut Driver<T>,
    peer: DeviceId,
) -> Result<&mut llts_core::session::PeerSession, NodeError> {
    driver
        .session_mut(&static_key(peer))
        .ok_or(NodeError::NotConnected(peer))
}

/// The class a [`DriverConfig`](llts_transport::session::DriverConfig) announces in its Hello.
pub const fn driver_class(class: DeviceClass) -> TransportClass {
    match class {
        DeviceClass::Phone => TransportClass::Phone,
        DeviceClass::Tablet => TransportClass::Tablet,
        DeviceClass::Computer => TransportClass::Laptop,
        DeviceClass::Watch => TransportClass::Other,
    }
}

/// Rounds a millihertz rate to whole hertz, never below one.
fn frame_rate(fps_mhz: u32) -> FrameRate {
    let hertz = u16::try_from(fps_mhz.div_ceil(1_000)).unwrap_or(u16::MAX);
    NonZeroU16::new(hertz).map_or(FrameRate::STANDARD, FrameRate::from_hz)
}
