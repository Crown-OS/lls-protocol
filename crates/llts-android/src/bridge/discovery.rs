use jni::EnvUnowned;
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jboolean, jlong};

use super::registry::instance;
use super::run;
use crate::command::{
    Command, NodeCommand, parse_beacon, parse_ip_addresses, parse_socket_addresses,
};

/// A beacon Kotlin's BLE scanner or NSD browser saw appear (`seen`) or disappear, with the
/// `"host:port"` addresses it can be dialled at as a JSON array (empty for BLE).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativePresence<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    beacon: JByteArray<'local>,
    addresses_json: JString<'local>,
    seen: jboolean,
) {
    run(&mut env, |env| {
        let beacon = parse_beacon(&env.convert_byte_array(&beacon)?)?;
        let addresses = parse_socket_addresses(&addresses_json.try_to_string(env)?)?;
        instance(handle)?.send(Command::Node(NodeCommand::Presence {
            beacon,
            addresses,
            seen,
        }))
    });
}

/// This device's IP addresses changed; `addresses_json` is a JSON array of them, without
/// link-local ones, which a peer could not use without a scope.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeNetworkChanged<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    addresses_json: JString<'local>,
) {
    run(&mut env, |env| {
        let addresses = parse_ip_addresses(&addresses_json.try_to_string(env)?)?;
        instance(handle)?.send(Command::Node(NodeCommand::NetworkChanged { addresses }))
    });
}

/// The beacons to advertise now, each behind a one-byte length; `null` when there are none.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeAdvertisements<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> JByteArray<'local> {
    run(&mut env, |env| {
        let encoded = instance(handle)?.advertisements();
        if encoded.is_empty() {
            return Ok(JByteArray::default());
        }
        Ok(env.byte_array_from_slice(&encoded)?)
    })
}
