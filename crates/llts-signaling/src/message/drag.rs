use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::message::limits::{
    MAX_DRAG_ENTER_BYTES, MAX_DRAG_ITEMS, MAX_DRAG_NAME_BYTES, MAX_MIME_BYTES,
};
use crate::message::{Edge, EdgePosition, Validate};
use crate::util::bounds;

/// Names one drag, allocated by the dragging side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DragId(pub u32);

tolerant_value! {
    /// What a dragged item is; items of a kind this build does not know are skipped.
    DragItemKind(u8) {
        TEXT = 0,
        IMAGE = 1,
        FILE = 2,
    }
}

/// One dragged item, described but not carried: text and images are fetched after the drop,
/// files go over ffsp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DragItem<'a> {
    pub kind: DragItemKind,
    pub mime: &'a str,
    #[serde(borrow)]
    pub name: Option<&'a str>,
    pub len: u64,
}

impl DragItem<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::text("drag item mime", self.mime, MAX_MIME_BYTES)?;
        self.name.map_or(Ok(()), |name| {
            bounds::file_name("drag item name", name, MAX_DRAG_NAME_BYTES)
        })
    }
}

/// A drag crossed the sender's `edge` at `position` and continues on the receiver.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DragEnter<'a> {
    pub drag: DragId,
    pub edge: Edge,
    pub position: EdgePosition,
    #[serde(borrow)]
    pub items: Vec<DragItem<'a>>,
}

impl Validate for DragEnter<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::count("drag items", self.items.len(), MAX_DRAG_ITEMS)?;
        self.items.iter().try_for_each(DragItem::validate)?;
        let len = postcard::experimental::serialized_size(self).map_err(SignalingError::Encode)?;
        if len > MAX_DRAG_ENTER_BYTES {
            return Err(SignalingError::FieldTooLong {
                field: "drag enter",
                len,
                limit: MAX_DRAG_ENTER_BYTES,
            });
        }
        Ok(())
    }
}

/// The user released the drag over the receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DragDrop {
    pub drag: DragId,
}

impl Validate for DragDrop {}

/// The drag ended without a drop, or went back to the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DragCancel {
    pub drag: DragId,
}

impl Validate for DragCancel {}
