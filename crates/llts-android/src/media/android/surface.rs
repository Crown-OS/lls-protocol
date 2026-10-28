use jni::Env;
use jni::objects::JObject;
use ndk::media::media_codec::MediaCodec;
use ndk::native_window::NativeWindow;

use super::codec_step;
use crate::media::MediaError;

/// An `ANativeWindow` a codec renders into or reads from.
#[derive(Debug)]
pub(crate) struct Surface(NativeWindow);

impl Surface {
    /// A persistent encoder input surface and its Java `Surface`. The NDK only accepts input
    /// surfaces it created itself, so this one is made here and handed to Kotlin rather than
    /// the other way round.
    pub(crate) fn encoder_input<'local>(
        env: &mut Env<'local>,
    ) -> Result<(Self, JObject<'local>), MediaError> {
        let window = MediaCodec::create_persistent_input_surface()
            .map_err(codec_step("creating the encoder surface"))?;
        // SAFETY: `env` is the JNIEnv of the native call in progress on this thread.
        let java = unsafe { window.to_surface(env.get_raw().cast()) };
        if java.is_null() {
            return Err(MediaError::NoWindow);
        }
        // SAFETY: `to_surface` returned a new local reference owned by this call's frame.
        let java = unsafe { JObject::from_raw(env, java.cast()) };
        Ok((Self(window), java))
    }

    /// The native window behind a Java `android.view.Surface`.
    pub(crate) fn from_java(env: &mut Env<'_>, surface: &JObject<'_>) -> Result<Self, MediaError> {
        if surface.as_raw().is_null() {
            return Err(MediaError::NoWindow);
        }
        // SAFETY: `env` is this call's JNIEnv and `surface` a live reference to the
        // `android.view.Surface` Kotlin passed, which the Kotlin signature guarantees.
        unsafe { NativeWindow::from_surface(env.get_raw().cast(), surface.as_raw().cast()) }
            .map(Self)
            .ok_or(MediaError::NoWindow)
    }

    pub(super) fn into_window(self) -> NativeWindow {
        self.0
    }
}
