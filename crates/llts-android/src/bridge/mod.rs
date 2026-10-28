//! The JNI surface of `com.crownos.connect.llts.Llts`.
//!
//! Every entry point runs inside jni's `with_env`, and every error becomes a Java exception
//! through [`policy::ThrowJavaException`]: `IllegalArgumentException` for a malformed argument,
//! `IllegalStateException` for a runtime that cannot do what was asked. Handles are keys into a
//! registry rather than pointers, so a stale or forged handle is an exception, never a crash.

pub mod discovery;
pub mod features;
pub mod lifecycle;
pub mod media;
pub mod pairing;
mod policy;
mod registry;
pub mod state;

use jni::{Env, EnvUnowned};

use crate::error::BridgeError;

/// Runs one entry point's body, turning its error or panic into a pending Java exception and
/// the return type's default.
fn run<'local, T: Default>(
    env: &mut EnvUnowned<'local>,
    body: impl FnOnce(&mut Env<'local>) -> Result<T, BridgeError>,
) -> T {
    env.with_env(body).resolve::<policy::ThrowJavaException>()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    /// Every `Llts.native*` method this library exports, in the order `Llts.kt` declares them.
    const EXPORTED_NATIVE_METHODS: [&str; 21] = [
        "nativeStart",
        "nativeStop",
        "nativeNextEvent",
        "nativeScanQr",
        "nativeBeginPairing",
        "nativeCancelPairing",
        "nativePairNearby",
        "nativeConfirmPairing",
        "nativeForget",
        "nativeSetFeature",
        "nativeStartFeature",
        "nativeStopFeature",
        "nativeSendCommand",
        "nativePublishStateJson",
        "nativePresence",
        "nativeNetworkChanged",
        "nativeAdvertisements",
        "nativeSendInput",
        "nativeCreateEncoderSurface",
        "nativeAttachDecoderSurface",
        "nativeDetachSurface",
    ];

    const SYMBOL_PREFIX: &str = "fn Java_com_crownos_connect_llts_Llts_";

    const SOURCES: [&str; 6] = [
        include_str!("discovery.rs"),
        include_str!("features.rs"),
        include_str!("lifecycle.rs"),
        include_str!("media.rs"),
        include_str!("pairing.rs"),
        include_str!("state.rs"),
    ];

    fn identifiers_after<'s>(text: &'s str, marker: &str) -> impl Iterator<Item = &'s str> {
        text.match_indices(marker).filter_map(move |(at, _)| {
            let rest = text.get(at + marker.len()..)?;
            let end = rest
                .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .unwrap_or(rest.len());
            rest.get(..end)
        })
    }

    #[test]
    fn every_listed_method_is_defined_exactly_once_and_nothing_else_is() {
        let defined: Vec<&str> = SOURCES
            .iter()
            .flat_map(|source| identifiers_after(source, SYMBOL_PREFIX))
            .collect();
        let unique: BTreeSet<&str> = defined.iter().copied().collect();
        assert_eq!(unique.len(), defined.len(), "a JNI symbol is defined twice");
        let listed: BTreeSet<&str> = EXPORTED_NATIVE_METHODS.into_iter().collect();
        assert_eq!(unique, listed);
    }

    /// Checked whenever the Android app sits next to this repository, as it does in CrownOS.
    #[test]
    fn the_kotlin_binding_declares_the_same_methods_in_the_same_order() {
        let kotlin = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../crownconnect-android/app/src/main/java/com/crownos/connect/llts/Llts.kt"
        );
        let Ok(source) = std::fs::read_to_string(kotlin) else {
            return;
        };
        let declared: Vec<&str> = identifiers_after(&source, "external fun ").collect();
        assert_eq!(declared, EXPORTED_NATIVE_METHODS);
    }
}
