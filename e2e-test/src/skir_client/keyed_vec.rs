use std::collections::HashMap;
use std::hash::Hash;
use std::ops::Deref;
use std::sync::OnceLock;

/// Extracts a lookup key from an element of type `T`.
pub trait GetKey<T> {
    type Key: Copy + Eq + Hash;
    fn get_key(item: &T) -> Self::Key;
}

/// An immutable vector that supports O(1) lookup by key.
///
/// The index is built lazily on the first call to [`KeyedVec::find_by_key`] and
/// cached for subsequent calls. Building and caching the index is thread-safe.
///
/// `G` must implement [`GetKey<T>`], which extracts the lookup key from an
/// element. When multiple elements share the same key, [`KeyedVec::find_by_key`]
/// returns the first one.
pub struct KeyedVec<T, G>
where
    G: GetKey<T>,
{
    items: Vec<T>,
    index: OnceLock<HashMap<G::Key, usize>>,
    _marker: std::marker::PhantomData<G>,
}

impl<T, G> KeyedVec<T, G>
where
    G: GetKey<T>,
{
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            index: OnceLock::new(),
            _marker: std::marker::PhantomData,
        }
    }

    /// Returns the first element whose key equals `key`, or `None`.
    ///
    /// The index is built on the first call and reused on all subsequent calls.
    pub fn find_by_key(&self, key: G::Key) -> Option<&T> {
        let index = self.index.get_or_init(|| {
            let mut map = HashMap::with_capacity(self.items.len());
            for (i, item) in self.items.iter().enumerate() {
                map.entry(G::get_key(item)).or_insert(i);
            }
            map
        });
        index.get(&key).map(|&i| &self.items[i])
    }
}

impl<T, G> Deref for KeyedVec<T, G>
where
    G: GetKey<T>,
{
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.items
    }
}

impl<'a, T, G> IntoIterator for &'a KeyedVec<T, G>
where
    G: GetKey<T>,
{
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Item {
        id: u32,
        name: &'static str,
    }

    struct ItemById;

    impl GetKey<Item> for ItemById {
        type Key = u32;
        fn get_key(item: &Item) -> u32 {
            item.id
        }
    }

    fn make_vec() -> KeyedVec<Item, ItemById> {
        KeyedVec::new(vec![
            Item { id: 1, name: "one" },
            Item { id: 2, name: "two" },
            Item { id: 3, name: "three" },
        ])
    }

    // --- find_by_key() ---

    #[test]
    fn search_existing_key_returns_element() {
        let kv = make_vec();
        let found = kv.find_by_key(2).expect("key 2 should be present");
        assert_eq!(found.name, "two");
    }

    #[test]
    fn search_missing_key_returns_none() {
        let kv = make_vec();
        assert!(kv.find_by_key(99).is_none());
    }

    #[test]
    fn search_duplicate_key_returns_first_occurrence() {
        let kv: KeyedVec<Item, ItemById> = KeyedVec::new(vec![
            Item { id: 42, name: "first" },
            Item { id: 42, name: "second" },
        ]);
        let found = kv.find_by_key(42).expect("key 42 should be present");
        assert_eq!(found.name, "first");
    }

    #[test]
    fn search_empty_vec_returns_none() {
        let kv: KeyedVec<Item, ItemById> = KeyedVec::new(vec![]);
        assert!(kv.find_by_key(1).is_none());
    }

    // Index is built once; calling find_by_key() a second time still works.
    #[test]
    fn search_called_twice_returns_same_result() {
        let kv = make_vec();
        assert!(kv.find_by_key(1).is_some());
        assert!(kv.find_by_key(1).is_some());
    }

    // --- Vec-like behaviour (Deref) ---

    #[test]
    fn len_reflects_number_of_items() {
        let kv = make_vec();
        assert_eq!(kv.len(), 3);
    }

    #[test]
    fn is_empty_false_for_non_empty_vec() {
        let kv = make_vec();
        assert!(!kv.is_empty());
    }

    #[test]
    fn is_empty_true_for_empty_vec() {
        let kv: KeyedVec<Item, ItemById> = KeyedVec::new(vec![]);
        assert!(kv.is_empty());
    }

    #[test]
    fn index_operator_accesses_elements() {
        let kv = make_vec();
        assert_eq!(kv[0].name, "one");
        assert_eq!(kv[2].name, "three");
    }

    #[test]
    fn iter_visits_all_elements_in_order() {
        let kv = make_vec();
        let names: Vec<&str> = kv.iter().map(|item| item.name).collect();
        assert_eq!(names, ["one", "two", "three"]);
    }

    // --- IntoIterator for &KeyedVec ---

    #[test]
    fn for_loop_visits_all_elements_in_order() {
        let kv = make_vec();
        let mut names = Vec::new();
        for item in &kv {
            names.push(item.name);
        }
        assert_eq!(names, ["one", "two", "three"]);
    }
}

