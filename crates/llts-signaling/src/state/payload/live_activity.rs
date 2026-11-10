use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::state::{NotificationAction, Topic, TopicPayload};
use crate::util::bounds;

pub const MAX_LIVE_ACTIVITIES: usize = 8;
pub const MAX_LIVE_ACTIONS: usize = 3;
/// The cap on every string of an activity and its actions; producers truncate UTF-8 safely.
pub const MAX_LIVE_FIELD_BYTES: usize = 256;
/// Producers drop their lowest-priority activities until the encoded document fits.
pub const MAX_LIVE_DOCUMENT_BYTES: usize = 24 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LiveProgress {
    pub value: u32,
    pub max: u32,
    pub indeterminate: bool,
}

/// A running clock the receiver animates itself, so the document need not change every second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Chronometer {
    pub base_unix_ms: u64,
    pub counts_down: bool,
}

/// One ongoing notification worth showing outside the phone: a timer, a delivery, a route.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LiveActivity<'a> {
    /// The platform's key for the notification, stable while it is shown.
    pub id: &'a str,
    pub package: &'a str,
    /// The app's label.
    pub app: &'a str,
    pub title: &'a str,
    pub text: &'a str,
    #[serde(borrow)]
    pub short_text: Option<&'a str>,
    /// The notification category verbatim, such as `navigation` or `stopwatch`.
    #[serde(borrow)]
    pub category: Option<&'a str>,
    pub progress: Option<LiveProgress>,
    pub chronometer: Option<Chronometer>,
    /// Promoted to the status bar by the platform, which ranks it first.
    pub promoted: bool,
    pub color_argb: Option<u32>,
    #[serde(borrow)]
    pub actions: Vec<NotificationAction<'a>>,
}

impl LiveActivity<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::count(
            "live activity actions",
            self.actions.len(),
            MAX_LIVE_ACTIONS,
        )?;
        let fields = [
            self.id,
            self.package,
            self.app,
            self.title,
            self.text,
            self.short_text.unwrap_or_default(),
            self.category.unwrap_or_default(),
        ];
        let action_fields = self
            .actions
            .iter()
            .flat_map(|action| [action.id, action.label]);
        fields
            .into_iter()
            .chain(action_fields)
            .try_for_each(|field| bounds::text("live activity field", field, MAX_LIVE_FIELD_BYTES))
    }
}

/// The phone's ongoing activities, most important first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LiveActivities<'a> {
    #[serde(borrow)]
    pub activities: Vec<LiveActivity<'a>>,
}

impl LiveActivities<'_> {
    /// Checks every cap except the encoded size, which only the bytes can tell.
    ///
    /// # Errors
    ///
    /// Returns the first cap this document breaks.
    pub fn validate(&self) -> Result<(), SignalingError> {
        bounds::count(
            "live activities",
            self.activities.len(),
            MAX_LIVE_ACTIVITIES,
        )?;
        self.activities.iter().try_for_each(LiveActivity::validate)
    }
}

impl<'a> TopicPayload<'a> for LiveActivities<'a> {
    const TOPIC: Topic = Topic::LiveActivities;
}
