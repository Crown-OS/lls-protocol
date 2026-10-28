use jni::EnvUnowned;
use jni::objects::{JClass, JString};
use jni::sys::jlong;

use super::registry::instance;
use super::run;
use crate::command::{Command, parse_topic};
use crate::state_json::encode_document;

/// Publishes this device's document for `topic` (a `Topic` variant name) from the JSON shape
/// `remoteState` events carry; it is checked and converted to postcard here, on the caller's
/// thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativePublishStateJson<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
    topic: JString<'local>,
    json: JString<'local>,
) {
    run(&mut env, |env| {
        let topic = parse_topic(&topic.try_to_string(env)?)?;
        let payload = encode_document(topic, &json.try_to_string(env)?)?;
        instance(handle)?.send(Command::PublishState { topic, payload })
    });
}
