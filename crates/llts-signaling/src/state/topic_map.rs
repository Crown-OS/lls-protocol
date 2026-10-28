use crate::state::Topic;

/// One optional slot per [`Topic`], with no hashing and no allocation of its own.
#[derive(Debug, Clone)]
pub(crate) struct TopicMap<T>([Option<T>; Topic::COUNT]);

impl<T> Default for TopicMap<T> {
    fn default() -> Self {
        Self(core::array::from_fn(|_| None))
    }
}

impl<T> TopicMap<T> {
    pub(crate) fn get(&self, topic: Topic) -> Option<&T> {
        self.0.get(topic.index()).and_then(Option::as_ref)
    }

    #[allow(
        clippy::indexing_slicing,
        reason = "Topic::index is below Topic::COUNT by construction"
    )]
    pub(crate) const fn slot(&mut self, topic: Topic) -> &mut Option<T> {
        &mut self.0[topic.index()]
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (Topic, &T)> {
        Topic::ALL
            .into_iter()
            .zip(&self.0)
            .filter_map(|(topic, slot)| slot.as_ref().map(|value| (topic, value)))
    }
}
