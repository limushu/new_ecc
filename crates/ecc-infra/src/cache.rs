use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::RwLock;

pub struct Cache<K, V>(RwLock<HashMap<K, V>>);

impl<K, V> Cache<K, V> {
    pub fn new() -> Self {
        Self(RwLock::new(HashMap::new()))
    }
}

impl<K: Eq + Hash + Clone, V: Clone> Cache<K, V> {
    pub fn get<Q: ?Sized>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.0.read().ok()?.get(key).cloned()
    }

    pub fn list_values(&self) -> Vec<V> {
        self.0
            .read()
            .map(|g| g.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn list_all(&self) -> HashMap<K, V> {
        self.0.read().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn set(&self, key: K, value: V) {
        if let Ok(mut w) = self.0.write() {
            w.insert(key, value);
        }
    }

    pub fn remove(&self, key: &K) {
        if let Ok(mut w) = self.0.write() {
            w.remove(key);
        }
    }

    pub fn clear_and_fill(&self, entries: HashMap<K, V>) {
        if let Ok(mut w) = self.0.write() {
            *w = entries;
        }
    }
}

pub type ProviderCache = Cache<String, ecc_domain::Provider>;
pub type RouteCache = Cache<String, Vec<ecc_domain::mapping::RouteTarget>>;
