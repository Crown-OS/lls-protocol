use std::net::SocketAddr;

use llts_core::session::Action;

use crate::TransportError;
use crate::security::{Hello, StaticKey, VerificationCode};

/// Something the driver needs the application to know or do.
#[derive(Debug)]
pub enum DriverEvent {
    /// A session with `peer` is up and carrying traffic.
    Connected { peer: StaticKey, hello: Hello },
    /// Nearby pairing finished its handshake. Show `code`; the user then confirms or rejects.
    PairingCode {
        peer: StaticKey,
        hello: Hello,
        code: VerificationCode,
    },
    /// `peer` is now in the trust store.
    Paired { peer: StaticKey },
    /// A session asked the application to do something: present a frame, deliver a message,
    /// retune an encoder.
    Session { peer: StaticKey, action: Action },
    /// A handshake this side started got no usable answer.
    HandshakeFailed {
        address: SocketAddr,
        error: TransportError,
    },
}
