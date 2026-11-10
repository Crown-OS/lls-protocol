use jni::EnvUnowned;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jlong};

use super::registry::instance;
use super::run;
use crate::signal_json::encode_signal;
use crate::util::device_hex::parse_device_hex;

/// Sends `peer` the application message `kind` (a `MessageType` name, e.g.
/// `"ListConversationsV1"`) with the body `bodyJson`, in the shape `signal` events carry.
///
/// Returns `true` once the message is queued. `false` is reserved for a full queue, after which
/// the caller waits for `writable`. A malformed body, or one too large for a single message,
/// throws `IllegalArgumentException`; a peer that is gone or does not share the message's feature
/// throws `IllegalStateException`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_link_llts_Llts_nativeSendSignal<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    peer: JString<'local>,
    kind: JString<'local>,
    body_json: JString<'local>,
) -> jboolean {
    run(&mut env, |env| {
        let peer = parse_device_hex(&peer.try_to_string(env)?)?;
        let message = encode_signal(&kind.try_to_string(env)?, &body_json.try_to_string(env)?)?;
        instance(handle)?.send_application(peer, message)?;
        Ok(true)
    })
}
