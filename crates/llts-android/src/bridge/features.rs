use jni::EnvUnowned;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jlong};

use super::registry::instance;
use super::run;
use crate::command::{
    Command, NodeCommand, parse_feature, parse_feature_request, parse_peer_command,
};
use crate::util::device_hex::parse_device_hex;

/// Allows or forbids `feature` (a `Feature` variant name) with `peer`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeSetFeature<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    peer: JString<'local>,
    feature: JString<'local>,
    enabled: jboolean,
) {
    run(&mut env, |env| {
        let peer = parse_device_hex(&peer.try_to_string(env)?)?;
        let feature = parse_feature(&feature.try_to_string(env)?)?;
        instance(handle)?.send(Command::Node(NodeCommand::SetFeature {
            peer,
            feature,
            enabled,
        }))
    });
}

/// Starts a media feature with `peer` from its JSON request, e.g. `{"Camera":{…}}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeStartFeature<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    peer: JString<'local>,
    request_json: JString<'local>,
) {
    run(&mut env, |env| {
        let peer = parse_device_hex(&peer.try_to_string(env)?)?;
        let request = parse_feature_request(&request_json.try_to_string(env)?)?;
        instance(handle)?.send(Command::Node(NodeCommand::StartFeature { peer, request }))
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeStopFeature<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    peer: JString<'local>,
    feature: JString<'local>,
) {
    run(&mut env, |env| {
        let peer = parse_device_hex(&peer.try_to_string(env)?)?;
        let feature = parse_feature(&feature.try_to_string(env)?)?;
        instance(handle)?.send(Command::Node(NodeCommand::StopFeature { peer, feature }))
    });
}

/// Sends `peer` a command to carry out, e.g. `{"Media":"Pause"}`, in the shape `remoteCommand`
/// events carry.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeSendCommand<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    peer: JString<'local>,
    command_json: JString<'local>,
) {
    run(&mut env, |env| {
        let peer = parse_device_hex(&peer.try_to_string(env)?)?;
        let command = parse_peer_command(&command_json.try_to_string(env)?)?;
        instance(handle)?.send(Command::Node(NodeCommand::Send { peer, command }))
    });
}
