use serde::{Deserialize, Serialize};

use crate::state::{Topic, TopicPayload};

/// A button on a notification, or an inline reply field when `accepts_text` is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NotificationAction<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub accepts_text: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Notification<'a> {
    /// The platform's key for the notification, stable while it is shown.
    pub id: &'a str,
    pub app: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    #[serde(borrow)]
    pub actions: Vec<NotificationAction<'a>>,
}

/// The notifications a device is currently showing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Notifications<'a> {
    #[serde(borrow)]
    pub active: Vec<Notification<'a>>,
}

impl<'a> TopicPayload<'a> for Notifications<'a> {
    const TOPIC: Topic = Topic::Notifications;
}
