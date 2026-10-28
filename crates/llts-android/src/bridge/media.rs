use jni::EnvUnowned;
use jni::objects::{JClass, JObject, JString};
use jni::sys::{jint, jlong};
use llts_node::MediaRole;

use super::registry::instance;
use super::run;
use crate::command::Command;
use crate::error::BridgeError;
use crate::input_json::parse_input_moment;
use crate::media::Surface;
use crate::util::device_hex::parse_device_hex;

/// Creates the input `Surface` for the encoder of video stream `index` to `peer`, which a
/// `mediaStart` event with the `Encoder` role announced. Kotlin hands it to MediaProjection's
/// virtual display or the camera; frames never leave the GPU until the encoder has them.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeCreateEncoderSurface<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    peer: JString<'local>,
    index: jint,
) -> JObject<'local> {
    run(&mut env, |env| {
        let peer = parse_device_hex(&peer.try_to_string(env)?)?;
        let index = stream_index(index)?;
        let instance = instance(handle)?;
        let (surface, java_surface) = Surface::encoder_input(env)?;
        instance.send(Command::AttachSurface {
            peer,
            index,
            role: MediaRole::Encoder,
            surface,
        })?;
        Ok(java_surface)
    })
}

/// Renders the decoder of video stream `index` from `peer` into `surface`, typically a
/// `SurfaceView`'s.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeAttachDecoderSurface<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    peer: JString<'local>,
    index: jint,
    surface: JObject<'local>,
) {
    run(&mut env, |env| {
        let peer = parse_device_hex(&peer.try_to_string(env)?)?;
        let index = stream_index(index)?;
        let surface = Surface::from_java(env, &surface)?;
        instance(handle)?.send(Command::AttachSurface {
            peer,
            index,
            role: MediaRole::Decoder,
            surface,
        })
    });
}

/// Stops the pipeline of video stream `index` with `peer`, as when its view goes away.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeDetachSurface<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    peer: JString<'local>,
    index: jint,
) {
    run(&mut env, |env| {
        let peer = parse_device_hex(&peer.try_to_string(env)?)?;
        let index = stream_index(index)?;
        instance(handle)?.send(Command::DetachSurface { peer, index })
    });
}

/// Sends one moment of input to `peer` on the session's input stream, for when this device
/// drives the peer's screen: `{"timeMs": …, "events": [...]}`, the shape `input` events have.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeSendInput<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    peer: JString<'local>,
    moment: JString<'local>,
) {
    run(&mut env, |env| {
        let peer = parse_device_hex(&peer.try_to_string(env)?)?;
        let batch = parse_input_moment(&moment.try_to_string(env)?)?;
        instance(handle)?.send(Command::SendInput { peer, batch })
    });
}

fn stream_index(index: jint) -> Result<u8, BridgeError> {
    u8::try_from(index).map_err(|_| BridgeError::StreamIndex(index))
}
