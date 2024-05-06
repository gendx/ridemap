//! Least recently used key-value store.

use log::debug;
use std::borrow::Borrow;
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;

/// Key-value store of bounded capacity, evicting least recently used items.
pub struct Lru<K, V> {
    /// Underlying store.
    map: HashMap<K, Item<V>>,
    /// Maximum capacity of the store.
    capacity: usize,
    /// Generation of the last item in the store.
    generation: Cell<usize>,
}

/// Item, associating a value with its generation number.
struct Item<V> {
    /// Generation number.
    generation: Cell<usize>,
    /// Value.
    value: V,
}

impl<K, V> Lru<K, V>
where
    K: Eq + Hash + Clone + Debug,
{
    /// Creates a store with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity),
            capacity,
            generation: Cell::new(0),
        }
    }

    /// Checks whether the store contains the given key.
    pub fn contains_key<Q>(&self, k: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        self.map.contains_key(k)
    }

    /// Obtains the item for the given key, making it the most recently used.
    pub fn get<Q>(&self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        match self.map.get(k) {
            Some(item) => {
                item.generation.set(self.next_generation());
                Some(&item.value)
            }
            None => None,
        }
    }

    /// Inserts the given value in the store, if the store doesn't already
    /// contain this key.
    ///
    /// Returns:
    /// 1. whether an item was inserted,
    /// 2. the evicted key, if any.
    pub fn or_insert_with(
        &mut self,
        k: K,
        priority: impl FnMut(&K) -> usize,
        default: impl FnOnce() -> Option<V>,
    ) -> (bool, Option<K>) {
        if self.map.contains_key(&k) {
            return (false, None);
        }

        if let Some(value) = default() {
            let evicted = if self.map.len() == self.capacity {
                self.evict(priority)
            } else {
                None
            };

            self.map.insert(
                k,
                Item {
                    generation: Cell::new(self.next_generation()),
                    value,
                },
            );
            (true, evicted)
        } else {
            (false, None)
        }
    }

    /// Evicts an item from the store, following:
    /// 1. the given priority predicate,
    /// 2. among items of equal priority, the least recently used item is
    /// evicted.
    fn evict<P: FnMut(&K) -> usize>(&mut self, mut priority: P) -> Option<K> {
        if let Some((oldest_key, _)) = self.map.iter().min_by(|(ka, a), (kb, b)| {
            priority(kb)
                .cmp(&priority(ka))
                .then(a.generation.get().cmp(&b.generation.get()))
        }) {
            debug!(
                "Evicting {:?} of priority {}",
                oldest_key,
                priority(oldest_key)
            );
            let oldest_key = oldest_key.clone();
            self.map.remove(&oldest_key);
            Some(oldest_key)
        } else {
            None
        }
    }

    /// Increments and returns the next generation number.
    fn next_generation(&self) -> usize {
        let new_generation = self.generation.get() + 1;
        self.generation.set(new_generation);
        new_generation
    }
}
