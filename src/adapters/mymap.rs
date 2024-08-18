use std::borrow::Borrow;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, MutexGuard};

use bustle::*;
use hashbrown::raw::RawTable;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::Value;

pub const fn ptr_size_bits() -> usize {
    std::mem::size_of::<usize>() * 8
}

fn ncb(shard_amount: usize) -> usize {
    shard_amount.trailing_zeros() as usize
}

pub trait Lock {
    type T;
    type ReadGuard<'a>: Deref<Target = Self::T>
    where
        Self: 'a;
    type WriteGuard<'a>: DerefMut<Target = Self::T>
    where
        Self: 'a;

    fn new(value: Self::T) -> Self;

    fn read(&self) -> Self::ReadGuard<'_>;

    fn write(&self) -> Self::WriteGuard<'_>;
}

impl<T> Lock for Mutex<T> {
    type T = T;

    type ReadGuard<'a> = MutexGuard<'a, Self::T> where T: 'a;

    type WriteGuard<'a> = MutexGuard<'a, Self::T> where T: 'a;

    fn new(value: Self::T) -> Self {
        Self::new(value)
    }

    fn read(&self) -> Self::ReadGuard<'_> {
        self.lock().unwrap()
    }

    fn write(&self) -> Self::WriteGuard<'_> {
        self.lock().unwrap()
    }
}

impl<T> Lock for RwLock<T> {
    type T = T;
    type ReadGuard<'a> = RwLockReadGuard<'a, Self::T> where T: 'a;

    type WriteGuard<'a> = RwLockWriteGuard<'a, Self::T> where T: 'a;

    fn new(value: Self::T) -> Self {
        Self::new(value)
    }

    fn read(&self) -> Self::ReadGuard<'_> {
        self.read().unwrap()
    }

    fn write(&self) -> Self::WriteGuard<'_> {
        self.write().unwrap()
    }
}

type DashLock<T> = lock_api::RwLock<super::lock::RawRwLock, T>;

impl<T> Lock for DashLock<T> {
    type T = T;
    type ReadGuard<'a> = lock_api::RwLockReadGuard<'a, super::lock::RawRwLock, Self::T> where T: 'a;

    type WriteGuard<'a> = lock_api::RwLockWriteGuard<'a, super::lock::RawRwLock, Self::T> where T: 'a;

    fn new(value: Self::T) -> Self {
        Self::new(value)
    }

    fn read(&self) -> Self::ReadGuard<'_> {
        self.read()
    }

    fn write(&self) -> Self::WriteGuard<'_> {
        self.write()
    }
}

type ParkingLock<T> = parking_lot::RwLock<T>;

impl<T> Lock for ParkingLock<T> {
    type T = T;
    type ReadGuard<'a> = parking_lot::RwLockReadGuard<'a, Self::T> where T: 'a;

    type WriteGuard<'a> = parking_lot::RwLockWriteGuard<'a, Self::T> where T: 'a;

    fn new(value: Self::T) -> Self {
        Self::new(value)
    }

    fn read(&self) -> Self::ReadGuard<'_> {
        self.read()
    }

    fn write(&self) -> Self::WriteGuard<'_> {
        self.write()
    }
}

#[repr(align(128))]
pub struct CachePadded<T>(T);

pub struct MyMap<L, S = RandomState, const C: usize = 7> {
    shift: usize,
    shards: Box<[CachePadded<L>]>,
    hasher: S,
}

impl<'a, K, V, L, S, const C: usize> MyMap<L, S, C>
where
    K: 'a + Eq + Hash,
    V: 'a,
    S: BuildHasher,
    L: Lock<T = RawTable<(K, V)>> + Send + Sync + 'static,
{
    pub fn with_capacity_and_hasher_and_shard_amount(
        mut capacity: usize,
        hasher: S,
        shard_amount: usize,
    ) -> Self {
        assert!(shard_amount > 1);
        assert!(shard_amount.is_power_of_two());

        let shift = ptr_size_bits() - ncb(shard_amount);

        if capacity != 0 {
            capacity = (capacity + (shard_amount - 1)) / shard_amount;
        }

        let cps = capacity / shard_amount;

        let shards = (0..shard_amount)
            .map(|_| CachePadded(L::new(RawTable::with_capacity(cps))))
            .collect();

        Self {
            shift,
            shards,
            hasher,
        }
    }

    fn get(&'a self, key: &K) -> bool {
        let (hash, shard) = self.shard(&key);

        let shard = shard.read();

        shard.get(hash, |(k, _v)| key == k).is_some()
    }

    fn put(&self, key: K, value: V) -> bool {
        let (hash, shard) = self.shard(&key);

        let mut shard = shard.write();

        unsafe {
            match shard.find_or_find_insert_slot(hash, |(k, _v)| k == &key, |(k, _v)| self.hash(&k))
            {
                Ok(bucket) => {
                    bucket.as_mut().1 = value;
                    false
                }
                Err(slot) => {
                    shard.insert_in_slot(hash, slot, (key, value));
                    true
                }
            }
        }
    }

    fn remove(&self, key: &K) -> bool {
        let (hash, shard) = self.shard(&key);

        let mut shard = shard.write();

        shard.remove_entry(hash, |(k, _v)| k == key).is_some()
    }

    fn shard(&self, key: &K) -> (u64, &L) {
        let hash = self.hash(key);

        let idx = self.determine_shard(hash as usize);

        let shard = &unsafe { self.shards.get_unchecked(idx) }.0;

        (hash, shard)
    }

    fn hash<T: Hash>(&self, item: &T) -> u64 {
        let mut hasher = self.hasher.build_hasher();

        item.hash(&mut hasher);

        hasher.finish()
    }

    pub fn determine_shard(&self, hash: usize) -> usize {
        (hash << C) >> self.shift
    }
}

pub struct MyMapTable<K, L, H, const C: usize>(Arc<MyMap<L, H, C>>)
where
    L: Lock<T = RawTable<(K, Value)>> + Send + Sync + 'static;

impl<K, L, H, const C: usize> Clone for MyMapTable<K, L, H, C>
where
    L: Lock<T = RawTable<(K, Value)>> + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub type MyMapMutexTable<K, H, const C: usize> = MyMapTable<K, Mutex<RawTable<(K, Value)>>, H, C>;
pub type MyMapRwLockTable<K, H, const C: usize> = MyMapTable<K, RwLock<RawTable<(K, Value)>>, H, C>;
pub type MyMapParkingLockTable<K, H, const C: usize> =
    MyMapTable<K, ParkingLock<RawTable<(K, Value)>>, H, C>;
pub type MyMapDashLockTable<K, H, const C: usize> =
    MyMapTable<K, DashLock<RawTable<(K, Value)>>, H, C>;

impl<K, L, H, const C: usize> Collection for MyMapTable<K, L, H, C>
where
    K: Send + Sync + From<u64> + Copy + 'static + Hash + Eq + std::fmt::Debug,
    H: BuildHasher + Default + Send + Sync + 'static,
    L: Lock<T = RawTable<(K, Value)>> + Send + Sync + 'static,
{
    type Handle = Self;

    fn with_capacity(capacity: usize) -> Self {
        Self(Arc::new(MyMap::with_capacity_and_hasher_and_shard_amount(
            capacity,
            H::default(),
            32,
        )))
    }

    fn pin(&self) -> Self::Handle {
        self.clone()
    }
}

impl<K, L, H, const C: usize> CollectionHandle for MyMapTable<K, L, H, C>
where
    K: Send + Sync + From<u64> + Copy + 'static + Hash + Eq + std::fmt::Debug,
    H: BuildHasher + Default + Send + Sync + 'static,
    L: Lock<T = RawTable<(K, Value)>> + Send + Sync + 'static,
{
    type Key = K;

    fn get(&mut self, key: &Self::Key) -> bool {
        self.0.get(key)
    }

    fn insert(&mut self, key: &Self::Key) -> bool {
        self.0.put(*key, 0)
    }

    fn remove(&mut self, key: &Self::Key) -> bool {
        self.0.remove(key)
    }

    fn update(&mut self, key: &Self::Key) -> bool {
        !self.0.put(*key, 0)
    }
}
