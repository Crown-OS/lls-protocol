#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test that fails a precondition should fail loudly"
)]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use llts_transport::TransportError;
use llts_transport::udp::{Outgoing, SessionSocket};

fn loopback() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
}

fn datagram(index: u16, len: usize) -> Vec<u8> {
    let mut bytes = vec![index.to_le_bytes()[0]; len];
    bytes[..2].copy_from_slice(&index.to_le_bytes());
    bytes
}

fn reachable(socket: &SessionSocket, ip: IpAddr) -> SocketAddr {
    SocketAddr::new(ip, socket.local_addr().unwrap().port())
}

#[test]
fn a_mixed_batch_arrives_whole_and_in_order() {
    let sender = SessionSocket::bind(loopback()).unwrap();
    let receiver = SessionSocket::bind(loopback()).unwrap();
    let (to, from) = (receiver.local_addr().unwrap(), sender.local_addr().unwrap());
    assert_mixed_batch_arrives(sender, receiver, to, from);
}

#[test]
fn a_dual_stack_socket_exchanges_ipv4_batches_under_plain_ipv4_addresses() {
    let dual_stack = || SessionSocket::bind(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))).unwrap();
    let ipv4 = || SessionSocket::bind(loopback()).unwrap();
    let localhost = IpAddr::V4(Ipv4Addr::LOCALHOST);

    let (sender, receiver) = (dual_stack(), ipv4());
    let (to, from) = (
        reachable(&receiver, localhost),
        reachable(&sender, localhost),
    );
    assert_mixed_batch_arrives(sender, receiver, to, from);

    let (sender, receiver) = (ipv4(), dual_stack());
    let (to, from) = (
        reachable(&receiver, localhost),
        reachable(&sender, localhost),
    );
    assert_mixed_batch_arrives(sender, receiver, to, from);
}

#[test]
fn a_dual_stack_socket_exchanges_ipv6_batches() {
    let localhost = IpAddr::V6(Ipv6Addr::LOCALHOST);
    let sender = SessionSocket::bind(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))).unwrap();
    let receiver = SessionSocket::bind(SocketAddr::new(localhost, 0)).unwrap();
    let (to, from) = (
        reachable(&receiver, localhost),
        reachable(&sender, localhost),
    );
    assert_mixed_batch_arrives(sender, receiver, to, from);
}

#[test]
fn a_refused_datagram_neither_resends_its_predecessors_nor_turns_gso_off() {
    let mut sender = SessionSocket::bind(loopback()).unwrap();
    let receiver = SessionSocket::bind(loopback()).unwrap();
    let gso_before = sender.gso_enabled();
    let good = receiver.local_addr().unwrap();
    let port_zero = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let ipv6_on_an_ipv4_socket = SocketAddr::from((Ipv6Addr::LOCALHOST, good.port()));
    let bytes = [7u8; 64];
    let outgoing = |to| Outgoing {
        bytes: &bytes,
        to,
        tos: 0xB8,
    };

    let batch = [outgoing(good), outgoing(port_zero), outgoing(good)];
    assert_eq!(sender.send_batch(&batch).unwrap(), 1);
    for refused in [port_zero, ipv6_on_an_ipv4_socket] {
        match sender.send_batch(&[outgoing(refused), outgoing(good)]) {
            Err(TransportError::SendFailed { to, .. }) => assert_eq!(to, refused),
            other => panic!("expected a refusal naming {refused}, got {other:?}"),
        }
    }
    assert_eq!(sender.gso_enabled(), gso_before);
    assert_eq!(sender.send_batch(&[outgoing(good)]).unwrap(), 1);
}

fn assert_mixed_batch_arrives(
    mut sender: SessionSocket,
    mut receiver: SessionSocket,
    to: SocketAddr,
    from: SocketAddr,
) {
    // A run of equal sizes GSO can coalesce, a short tail, then differently marked traffic.
    let mut payloads: Vec<Vec<u8>> = (0..24).map(|index| datagram(index, 1_200)).collect();
    payloads.push(datagram(24, 700));
    payloads.extend((25..40).map(|index| datagram(index, 90)));
    let outgoing: Vec<Outgoing<'_>> = payloads
        .iter()
        .enumerate()
        .map(|(index, bytes)| Outgoing {
            bytes,
            to,
            tos: if index < 25 { 0x88 } else { 0xB8 },
        })
        .collect();

    let mut sent = 0;
    while sent < outgoing.len() {
        sent += sender.send_batch(&outgoing[sent..]).unwrap();
    }

    let mut received = Vec::new();
    for _ in 0..200 {
        if received.len() == payloads.len() {
            break;
        }
        receiver
            .wait_readable(Some(Duration::from_millis(20)))
            .unwrap();
        receiver.receive_batch().unwrap();
        for (bytes, source) in receiver.received_mut() {
            assert_eq!(source, from);
            received.push(bytes.to_vec());
        }
    }
    assert_eq!(received, payloads);
}

#[test]
fn an_idle_socket_returns_nothing_without_blocking() {
    let mut socket = SessionSocket::bind(loopback()).unwrap();
    assert_eq!(socket.receive_batch().unwrap(), 0);
    assert!(
        !socket
            .wait_readable(Some(Duration::from_millis(1)))
            .unwrap()
    );
}
