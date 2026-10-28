use jni::Env;
use jni::errors::ErrorPolicy;
use jni::jni_str;
use jni::strings::JNIString;

use crate::error::{BridgeError, describe};

/// Throws the Java exception that matches a [`BridgeError`] and returns the default value.
#[derive(Debug, Default)]
pub(super) struct ThrowJavaException;

impl<T: Default> ErrorPolicy<T, BridgeError> for ThrowJavaException {
    type Captures<'unowned_env_local: 'native_method, 'native_method> = ();

    fn on_error<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _captures: &mut Self::Captures<'unowned_env_local, 'native_method>,
        error: BridgeError,
    ) -> jni::errors::Result<T> {
        if env.exception_check() {
            return Ok(T::default());
        }
        let class = if error.is_caller_mistake() {
            jni_str!("java/lang/IllegalArgumentException")
        } else {
            jni_str!("java/lang/IllegalStateException")
        };
        let message = JNIString::from(describe(&error));
        let _ = env.throw_new(class, &message);
        Ok(T::default())
    }

    fn on_panic<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _captures: &mut Self::Captures<'unowned_env_local, 'native_method>,
        _payload: Box<dyn std::any::Any + Send + 'static>,
    ) -> jni::errors::Result<T> {
        let _ = env.throw_new(
            jni_str!("java/lang/IllegalStateException"),
            jni_str!("llts panicked; the runtime can no longer be trusted"),
        );
        Ok(T::default())
    }
}
