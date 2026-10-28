/// A map with a fixed ceiling on entries, allocated once and searched linearly.
///
/// Sessions hold a handful of streams, so a scan over a contiguous array beats hashing, and the
/// ceiling means a peer inventing identifiers can never grow it.
#[derive(Debug)]
pub(crate) struct BoundedMap<K, V, const N: usize> {
    entries: Vec<(K, V)>,
}

impl<K: Copy + Eq, V, const N: usize> BoundedMap<K, V, N> {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::with_capacity(N),
        }
    }

    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        self.entries
            .iter()
            .find_map(|(candidate, value)| (candidate == key).then_some(value))
    }

    pub(crate) fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.entries
            .iter_mut()
            .find_map(|(candidate, value)| (candidate == key).then_some(value))
    }

    /// The entry for `key`, created by `make` if absent, or `None` once the map is full.
    pub(crate) fn get_or_insert_with(
        &mut self,
        key: K,
        make: impl FnOnce() -> V,
    ) -> Option<&mut V> {
        let position = match self
            .entries
            .iter()
            .position(|(candidate, _)| *candidate == key)
        {
            Some(position) => position,
            None if self.entries.len() < N => {
                self.entries.push((key, make()));
                self.entries.len() - 1
            }
            None => return None,
        };
        self.entries.get_mut(position).map(|(_, value)| value)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|(key, value)| (key, value))
    }
}

#[cfg(test)]
mod tests {
    use super::BoundedMap;

    #[test]
    fn inserts_stop_at_the_ceiling() {
        let mut map: BoundedMap<u8, u8, 2> = BoundedMap::new();
        assert!(map.get_or_insert_with(1, || 10).is_some());
        assert!(map.get_or_insert_with(2, || 20).is_some());
        assert!(map.get_or_insert_with(3, || 30).is_none());
        assert_eq!(map.get_or_insert_with(1, || 99).copied(), Some(10));
        assert_eq!(map.get(&2).copied(), Some(20));
    }
}
