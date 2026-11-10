use crate::SignalingError;

/// Refuses a string longer than `limit` bytes, naming the field but never its content.
pub(crate) const fn text(
    field: &'static str,
    text: &str,
    limit: usize,
) -> Result<(), SignalingError> {
    if text.len() > limit {
        return Err(SignalingError::FieldTooLong {
            field,
            len: text.len(),
            limit,
        });
    }
    Ok(())
}

pub(crate) const fn optional_text(
    field: &'static str,
    text: Option<&str>,
    limit: usize,
) -> Result<(), SignalingError> {
    match text {
        Some(text) => self::text(field, text, limit),
        None => Ok(()),
    }
}

pub(crate) const fn count(
    field: &'static str,
    count: usize,
    limit: usize,
) -> Result<(), SignalingError> {
    if count > limit {
        return Err(SignalingError::TooManyEntries {
            field,
            count,
            limit,
        });
    }
    Ok(())
}

/// Refuses a file name that is not a plain basename, which the receiver could otherwise be led
/// to write outside the directory it chose.
pub(crate) fn file_name(
    field: &'static str,
    name: &str,
    limit: usize,
) -> Result<(), SignalingError> {
    text(field, name, limit)?;
    let plain =
        !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0']);
    if plain {
        Ok(())
    } else {
        Err(SignalingError::OutOfRange { field })
    }
}
