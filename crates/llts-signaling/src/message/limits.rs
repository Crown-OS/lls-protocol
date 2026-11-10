//! Caps every application message keeps to, so each fits one reliable message and a receiver
//! can bound what it buffers. Producers truncate or omit to stay within them; receivers refuse
//! anything that breaks them.

/// Sum of the encoded item sizes of one page; the envelope stays below the reliable-message
/// limit.
pub const PAGE_BUDGET_BYTES: usize = 48 * 1024;
/// The hard cap on `PageRequest::limit`; responders clamp to it.
pub const MAX_PAGE_ITEMS: u16 = 200;
/// Message bodies and voicemail transcripts; longer ones are cut and flagged as truncated.
pub const MAX_INLINE_TEXT_BYTES: usize = 8 * 1024;
/// Page cursors, blob keys and the other opaque keys a peer hands back.
pub const MAX_CURSOR_BYTES: usize = 256;
pub const MAX_SNIPPET_BYTES: usize = 512;
/// Per conversation page item and per sent message.
pub const MAX_PARTICIPANTS: usize = 32;
/// Phones and emails of one contact, each.
pub const MAX_CONTACT_FIELDS: usize = 16;
pub const MAX_ATTACHMENTS_PER_ITEM: usize = 16;
pub const MAX_SEND_TEXT_BYTES: usize = 32 * 1024;
pub const MAX_URI_BYTES: usize = 2048;
pub const MAX_DRAG_ITEMS: usize = 64;
/// File names, which are basenames.
pub const MAX_DRAG_NAME_BYTES: usize = 255;
/// The encoded `DragEnter`; producers drop trailing items beyond it.
pub const MAX_DRAG_ENTER_BYTES: usize = 16 * 1024;
/// Text and image bytes one drag may carry, as the dragging side snapshots them.
pub const MAX_DRAG_PAYLOAD_BYTES: u64 = 64 * 1024 * 1024;
pub const BLOB_CHUNK_BYTES: usize = 48 * 1024;
/// Transport-queued bytes allowed on a peer's Blob lane, all blobs together.
pub const BLOB_LANE_WINDOW_BYTES: usize = 512 * 1024;
pub const BLOB_ACK_EVERY_CHUNKS: u64 = 8;
pub const MAX_BLOB_BYTES: u64 = 256 * 1024 * 1024;
/// Application messages waiting for transport capacity, per peer and lane.
pub const MAX_OUTBOX_BYTES: usize = 2 * 1024 * 1024;
/// A failure's `detail`, which never carries user data.
pub const MAX_DETAIL_BYTES: usize = 256;
/// A MIME type: 127 bytes of type, a slash and 127 of subtype (RFC 6838).
pub const MAX_MIME_BYTES: usize = 255;
/// The 802.11 limit on an SSID.
pub const MAX_SSID_BYTES: usize = 32;
/// Wi-Fi passphrases: 63 characters for WPA2, longer SAE passwords for WPA3.
pub const MAX_PASSPHRASE_BYTES: usize = 256;
/// Schemes a peer may ask this device to open; every other scheme is refused on both ends.
pub const ALLOWED_URI_SCHEMES: [&str; 7] =
    ["http", "https", "mailto", "geo", "tel", "sms", "smsto"];
