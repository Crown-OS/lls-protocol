use serde::de::DeserializeOwned;
use serde::de::IntoDeserializer;
use serde::de::value::Error as NameError;

use crate::error::BridgeError;

/// Reads a unit enum variant, such as a `Feature` or a `Topic`, by its Rust name: the same
/// spelling the JSON events use, so Kotlin has one set of names for both directions.
pub(crate) fn parse_name<T: DeserializeOwned>(
    what: &'static str,
    name: &str,
) -> Result<T, BridgeError> {
    T::deserialize(IntoDeserializer::<NameError>::into_deserializer(name)).map_err(|_| {
        BridgeError::UnknownName {
            what,
            name: name.to_owned(),
        }
    })
}

#[cfg(test)]
mod tests {
    use llts_signaling::device::Feature;
    use llts_signaling::state::Topic;

    use super::*;

    #[test]
    fn reads_rust_variant_names_only() {
        assert_eq!(
            parse_name::<Feature>("feature", "Mirror").ok(),
            Some(Feature::Mirror)
        );
        assert_eq!(
            parse_name::<Topic>("topic", "Battery").ok(),
            Some(Topic::Battery)
        );
        assert!(parse_name::<Feature>("feature", "mirror").is_err());
    }
}
