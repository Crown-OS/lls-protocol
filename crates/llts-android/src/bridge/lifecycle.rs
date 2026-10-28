use std::path::PathBuf;

use jni::EnvUnowned;
use jni::objects::{JClass, JString};
use jni::sys::jlong;

use super::registry::{instance, register, unregister};
use super::run;
use crate::config::StartConfig;
use crate::runtime::Instance;

/// Starts a runtime from a JSON [`StartConfig`], keeping its identity and trust store under
/// `filesDir`, and returns its handle.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeStart<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    config_json: JString<'local>,
    files_dir: JString<'local>,
) -> jlong {
    #[cfg(target_os = "android")]
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("llts"),
    );
    run(&mut env, |env| {
        let config = StartConfig::parse(&config_json.try_to_string(env)?)?;
        let files_dir = PathBuf::from(files_dir.try_to_string(env)?);
        Ok(register(Instance::start(&config, &files_dir)?))
    })
}

/// Stops the runtime and waits for its thread. A `nativeNextEvent` blocked on this handle then
/// returns `null`; the handle names nothing afterwards.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeStop<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    run(&mut env, |_| {
        if let Some(instance) = unregister(handle) {
            instance.stop();
        }
        Ok(())
    });
}

/// Blocks until the runtime has an event, and returns it as JSON; `null` once it has stopped.
/// Call it from one dedicated thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_crownos_connect_llts_Llts_nativeNextEvent<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> JString<'local> {
    run(&mut env, |env| {
        let Ok(instance) = instance(handle) else {
            return Ok(JString::default());
        };
        match instance.next_event() {
            Some(event) => Ok(JString::from_str(env, &event)?),
            None => Ok(JString::default()),
        }
    })
}
