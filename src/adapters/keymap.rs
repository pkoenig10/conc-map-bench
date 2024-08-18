use std::borrow::Borrow;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hash, Hasher};

use hashbrown::raw::RawTable;

pub trait Keyed {
    type Key;

    // Is this flexible enough?
    fn key(&self) -> &Self::Key;
}

impl<K, V> Keyed for (K, V) {
    type Key = K;

    fn key(&self) -> &K {
        &self.0
    }
}

pub trait Equivalent<K: ?Sized> {
    fn equivalent(&self, key: &K) -> bool;
}

impl<Q, K> Equivalent<K> for Q
where
    Q: Eq,
    K: Borrow<Q>,
{
    fn equivalent(&self, key: &K) -> bool {
        self == key.borrow()
    }
}

struct KeyMap<T, S = RandomState> {
    table: RawTable<T>,
    hash_builder: S,
}

impl<T, S> KeyMap<T, S>
where
    T: Keyed,
    T::Key: Hash,
    S: BuildHasher,
{
    fn get<K>(&self, key: &K) -> Option<&T>
    where
        K: Hash + Equivalent<T::Key>,
    {
        let hash = self.hash(key);

        self.table.get(hash, Self::make_eq(key))
    }

    fn insert(&mut self, value: T) {
        let hash = self.hash(value.key());

        self.table
            .insert(hash, value, Self::make_hasher(&self.hash_builder));
    }

    fn remove<K>(&mut self, key: &K)
    where
        K: Hash + Equivalent<T::Key>,
    {
        let hash = self.hash(key);

        self.table.remove_entry(hash, Self::make_eq(key));
    }

    fn hash<K>(&self, key: &K) -> u64
    where
        K: Hash,
    {
        Self::make_hash(&self.hash_builder, key)
    }

    #[inline]
    fn make_eq<K>(key: &K) -> impl Fn(&T) -> bool + '_
    where
        K: Equivalent<T::Key>,
    {
        move |v| key.equivalent(v.key())
    }

    #[inline]
    fn make_hasher(hash_builder: &S) -> impl Fn(&T) -> u64 + '_
    where
        S: BuildHasher,
    {
        move |v| Self::make_hash(hash_builder, v.key())
    }

    #[inline]
    fn make_hash<K>(hash_builder: &S, key: &K) -> u64
    where
        K: Hash,
    {
        let mut hasher = hash_builder.build_hasher();
        key.hash(&mut hasher);
        hasher.finish()
    }
}
