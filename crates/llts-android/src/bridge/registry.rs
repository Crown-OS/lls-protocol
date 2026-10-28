use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use jni::sys::jlong;

use crate::error::BridgeError;
use crate::runtime::Instance;

/// Running instances by handle. Handles count up from 1 and are never reused, so a handle Kotlin
/// kept after `nativeStop` names nothing rather than a newer instance.
static INSTANCES: Mutex<Vec<(jlong, Arc<Instance>)>> = Mutex::new(Vec::new());
static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);

pub(super) fn register(instance: Instance) -> jlong {
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    instances().push((handle, Arc::new(instance)));
    handle
}

/// The instance behind `handle`, kept alive by the caller even if another thread stops it.
pub(super) fn instance(handle: jlong) -> Result<Arc<Instance>, BridgeError> {
    instances()
        .iter()
        .find(|(registered, _)| *registered == handle)
        .map(|(_, instance)| Arc::clone(instance))
        .ok_or(BridgeError::NotRunning)
}

pub(super) fn unregister(handle: jlong) -> Option<Arc<Instance>> {
    let mut instances = instances();
    let position = instances
        .iter()
        .position(|(registered, _)| *registered == handle)?;
    Some(instances.swap_remove(position).1)
}

fn instances() -> std::sync::MutexGuard<'static, Vec<(jlong, Arc<Instance>)>> {
    INSTANCES.lock().unwrap_or_else(PoisonError::into_inner)
}
