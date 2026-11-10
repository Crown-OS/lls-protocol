use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::message::limits::{MAX_CONTACT_FIELDS, MAX_CURSOR_BYTES};
use crate::message::{PageRequest, RequestId, Validate};
use crate::util::bounds;

tolerant_value! {
    /// What a number is for, the platform's `TYPE_*` number; an unknown one shows its label or
    /// "Other".
    PhoneKind(u8) {
        HOME = 1,
        MOBILE = 2,
        WORK = 3,
        OTHER = 7,
        MAIN = 12,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContactPhone<'a> {
    pub number: &'a str,
    pub kind: PhoneKind,
    /// The user's own label for a custom kind.
    #[serde(borrow)]
    pub label: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Contact<'a> {
    /// The platform's stable lookup key.
    pub contact_key: &'a str,
    pub name: &'a str,
    pub starred: bool,
    #[serde(borrow)]
    pub phones: Vec<ContactPhone<'a>>,
    #[serde(borrow)]
    pub emails: Vec<&'a str>,
    #[serde(borrow)]
    pub organization: Option<&'a str>,
    #[serde(borrow)]
    pub photo_key: Option<&'a str>,
}

impl Contact<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::text("contact key", self.contact_key, MAX_CURSOR_BYTES)?;
        bounds::count("contact phones", self.phones.len(), MAX_CONTACT_FIELDS)?;
        bounds::count("contact emails", self.emails.len(), MAX_CONTACT_FIELDS)?;
        bounds::optional_text("contact photo key", self.photo_key, MAX_CURSOR_BYTES)
    }
}

/// Asks for a page of contacts, by display name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ListContacts<'a> {
    pub request: RequestId,
    #[serde(borrow)]
    pub page: PageRequest<'a>,
}

impl Validate for ListContacts<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        self.page.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContactPage<'a> {
    pub request: RequestId,
    #[serde(borrow)]
    pub items: Vec<Contact<'a>>,
    #[serde(borrow)]
    pub next: Option<&'a str>,
}

impl Validate for ContactPage<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::optional_text("page cursor", self.next, MAX_CURSOR_BYTES)?;
        self.items.iter().try_for_each(Contact::validate)
    }
}
