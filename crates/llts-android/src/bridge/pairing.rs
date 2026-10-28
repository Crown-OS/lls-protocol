use core::time::Duration;
use std::net::SocketAddr;

use jni::EnvUnowned;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jlong};

use super::registry::instance;
use super::run;
use crate::command::{Command, NodeCommand};
use crate::util::device_hex::parse_device_hex;

/// Pairs with the device whose QR offer the camera read.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeScanQr<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    qr: JString<'local>,
) {
    run(&mut env, |env| {
        let uri = qr.try_to_string(env)?;
        instance(handle)?.send(Command::Node(NodeCommand::PairWithQr { uri }))
    });
}

/// Opens a pairing window for `seconds`, or the default two minutes when it is not positive;
/// the QR offer arrives as a `pairingOffer` event.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeBeginPairing<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    seconds: jint,
) {
    run(&mut env, |_| {
        let duration = u64::try_from(seconds)
            .ok()
            .filter(|seconds| *seconds > 0)
            .map(Duration::from_secs);
        instance(handle)?.send(Command::Node(NodeCommand::BeginPairing { duration }))
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeCancelPairing<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    run(&mut env, |_| {
        instance(handle)?.send(Command::Node(NodeCommand::CancelPairing))
    });
}

/// Starts nearby pairing with a device a `pairableDeviceSeen` event reported at `address`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativePairNearby<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    address: JString<'local>,
) {
    run(&mut env, |env| {
        let address: SocketAddr = address.try_to_string(env)?.parse()?;
        instance(handle)?.send(Command::Node(NodeCommand::PairNearby { address }))
    });
}

/// The user compared the code of a `pairingRequest` and accepted or rejected it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeConfirmPairing<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    peer: JString<'local>,
    accept: jboolean,
) {
    run(&mut env, |env| {
        let peer = parse_device_hex(&peer.try_to_string(env)?)?;
        instance(handle)?.send(Command::Node(NodeCommand::ConfirmPairing { peer, accept }))
    });
}

/// Unpairs `peer` on both ends.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeForget<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    peer: JString<'local>,
) {
    run(&mut env, |env| {
        let peer = parse_device_hex(&peer.try_to_string(env)?)?;
        instance(handle)?.send(Command::Node(NodeCommand::Forget { peer }))
    });
}
