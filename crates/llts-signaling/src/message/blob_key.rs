use core::fmt;

use serde::Serialize;

use crate::SignalingError;
use crate::device::Feature;
use crate::message::DragId;
use crate::message::limits::MAX_CURSOR_BYTES;

/// What a blob key names. Keys travel as strings, but both ends build and read them only
/// through this type: ids are decimal, so nothing a peer sends is ever spliced into a content
/// URI, and each key carries a date or photo id, so a reused row id never serves stale bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum BlobKey {
    /// `sms-text/<id>/<date_ms>`
    SmsText { id: u64, date_ms: u64 },
    /// `mms-part/<part_id>/<date_ms>`, with the date of the owning MMS.
    MmsPart { part_id: u64, date_ms: u64 },
    /// `mms-thumb/<part_id>/<date_ms>/<edge>`
    MmsThumb {
        part_id: u64,
        date_ms: u64,
        edge: u16,
    },
    /// `contact-photo/<contact_id>/<photo_id>`
    ContactPhoto { contact_id: u64, photo_id: u64 },
    /// `voicemail/<id>/<date_ms>`
    Voicemail { id: u64, date_ms: u64 },
    /// `drag/<drag>/<index>`
    Drag { drag: DragId, index: u16 },
}

impl BlobKey {
    /// The longest edge a thumbnail may be asked for.
    pub const MAX_THUMB_EDGE: u16 = 1024;

    /// # Errors
    ///
    /// Returns [`SignalingError::MalformedBlobKey`] for a key over [`MAX_CURSOR_BYTES`], an
    /// unknown namespace, a number that is not canonical decimal, or a thumbnail edge outside
    /// `1..=MAX_THUMB_EDGE`.
    pub fn parse(key: &str) -> Result<Self, SignalingError> {
        if key.len() > MAX_CURSOR_BYTES {
            return Err(SignalingError::MalformedBlobKey);
        }
        let mut parts = key.split('/');
        let namespace = parts.next().ok_or(SignalingError::MalformedBlobKey)?;
        let mut number = || decimal(parts.next());
        let parsed = match namespace {
            "sms-text" => Self::SmsText {
                id: number()?,
                date_ms: number()?,
            },
            "mms-part" => Self::MmsPart {
                part_id: number()?,
                date_ms: number()?,
            },
            "mms-thumb" => Self::MmsThumb {
                part_id: number()?,
                date_ms: number()?,
                edge: narrow(number()?)
                    .filter(|edge| (1..=Self::MAX_THUMB_EDGE).contains(edge))
                    .ok_or(SignalingError::MalformedBlobKey)?,
            },
            "contact-photo" => Self::ContactPhoto {
                contact_id: number()?,
                photo_id: number()?,
            },
            "voicemail" => Self::Voicemail {
                id: number()?,
                date_ms: number()?,
            },
            "drag" => Self::Drag {
                drag: DragId(narrow(number()?).ok_or(SignalingError::MalformedBlobKey)?),
                index: narrow(number()?).ok_or(SignalingError::MalformedBlobKey)?,
            },
            _ => return Err(SignalingError::MalformedBlobKey),
        };
        if parts.next().is_some() {
            return Err(SignalingError::MalformedBlobKey);
        }
        Ok(parsed)
    }

    /// The feature a peer must share for this key to be served.
    pub const fn feature(&self) -> Feature {
        match self {
            Self::SmsText { .. } | Self::MmsPart { .. } | Self::MmsThumb { .. } => {
                Feature::Messages
            }
            Self::ContactPhoto { .. } => Feature::Contacts,
            Self::Voicemail { .. } => Feature::CallHistory,
            Self::Drag { .. } => Feature::DragDrop,
        }
    }
}

impl fmt::Display for BlobKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SmsText { id, date_ms } => write!(formatter, "sms-text/{id}/{date_ms}"),
            Self::MmsPart { part_id, date_ms } => {
                write!(formatter, "mms-part/{part_id}/{date_ms}")
            }
            Self::MmsThumb {
                part_id,
                date_ms,
                edge,
            } => write!(formatter, "mms-thumb/{part_id}/{date_ms}/{edge}"),
            Self::ContactPhoto {
                contact_id,
                photo_id,
            } => write!(formatter, "contact-photo/{contact_id}/{photo_id}"),
            Self::Voicemail { id, date_ms } => write!(formatter, "voicemail/{id}/{date_ms}"),
            Self::Drag { drag, index } => write!(formatter, "drag/{}/{index}", drag.0),
        }
    }
}

/// A canonical decimal: digits only, no sign, no leading zero, so every number has one
/// spelling and a key survives a parse and print unchanged.
fn decimal(text: Option<&str>) -> Result<u64, SignalingError> {
    let text = text.ok_or(SignalingError::MalformedBlobKey)?;
    let canonical = !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if !canonical {
        return Err(SignalingError::MalformedBlobKey);
    }
    text.parse().map_err(|_| SignalingError::MalformedBlobKey)
}

fn narrow<T: TryFrom<u64>>(value: u64) -> Option<T> {
    T::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_prints_as_it_parses() {
        for key in [
            "sms-text/12/1700000000000",
            "mms-part/3/1700000000000",
            "mms-thumb/3/1700000000000/256",
            "contact-photo/0/41",
            "voicemail/9/1",
            "drag/4294967295/63",
        ] {
            let parsed = BlobKey::parse(key);
            assert_eq!(parsed.map(|parsed| parsed.to_string()), Ok(key.to_owned()));
        }
    }

    #[test]
    fn keys_name_the_feature_that_serves_them() {
        assert_eq!(
            BlobKey::parse("mms-thumb/1/2/3").map(|key| key.feature()),
            Ok(Feature::Messages)
        );
        assert_eq!(
            BlobKey::parse("contact-photo/1/2").map(|key| key.feature()),
            Ok(Feature::Contacts)
        );
        assert_eq!(
            BlobKey::parse("voicemail/1/2").map(|key| key.feature()),
            Ok(Feature::CallHistory)
        );
        assert_eq!(
            BlobKey::parse("drag/1/2").map(|key| key.feature()),
            Ok(Feature::DragDrop)
        );
    }

    #[test]
    fn anything_but_canonical_keys_is_refused() {
        let long = format!("sms-text/1/{}", "1".repeat(MAX_CURSOR_BYTES));
        for key in [
            "",
            "sms-text",
            "sms-text/1",
            "sms-text/1/2/3",
            "sms-text/+1/2",
            "sms-text/01/2",
            "sms-text/-1/2",
            "sms-text/1/ 2",
            "sms-text/18446744073709551616/2",
            "mms-thumb/1/2/0",
            "mms-thumb/1/2/1025",
            "drag/4294967296/0",
            "drag/1/65536",
            "content://sms/1",
            "../sms-text/1/2",
            long.as_str(),
        ] {
            assert_eq!(
                BlobKey::parse(key),
                Err(SignalingError::MalformedBlobKey),
                "{key}"
            );
        }
    }
}
