/// Least-recently-used map holding at most `CAP` entries; [`Lru::get`] and
/// [`Lru::insert`] both mark the entry most recently used.
#[derive(Clone)]
pub struct Lru<K, V, const CAP: usize> {
    slots: Vec<Slot<K, V>>,
    /// Slot indices sorted by key.
    by_key: Vec<usize>,
    /// Most recently used slot.
    head: Option<usize>,
    /// Least recently used slot, the next one to evict.
    tail: Option<usize>,
}

#[derive(Clone)]
struct Slot<K, V> {
    key: K,
    value: V,
    prev: Option<usize>,
    next: Option<usize>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum InsertReplaced<K, V> {
    LruEvicted(K, V),
    OldValue(V),
}

impl<K: Ord, V, const CAP: usize> Default for Lru<K, V, CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord, V, const CAP: usize> Lru<K, V, CAP> {
    pub fn new() -> Self {
        Self {
            slots: Vec::with_capacity(CAP),
            by_key: Vec::with_capacity(CAP),
            head: None,
            tail: None,
        }
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        let slot = self.by_key[self.find(key).ok()?];
        self.touch(slot);
        Some(&self.slots[slot].value)
    }

    /// With `CAP == 0` nothing is stored and `None` is returned.
    pub fn insert(&mut self, key: K, value: V) -> Option<InsertReplaced<K, V>> {
        if CAP == 0 {
            return None;
        }
        let pos = match self.find(&key) {
            Ok(pos) => {
                let slot = self.by_key[pos];
                self.touch(slot);
                let old = std::mem::replace(&mut self.slots[slot].value, value);
                return Some(InsertReplaced::OldValue(old));
            }
            Err(pos) => pos,
        };

        if self.slots.len() < CAP {
            let slot = self.slots.len();
            self.slots.push(Slot {
                key,
                value,
                prev: None,
                next: None,
            });
            self.by_key.insert(pos, slot);
            self.link_front(slot);
            return None;
        }

        let slot = self.tail.expect("full lru must have a tail");
        let evicted_pos = self
            .find(&self.slots[slot].key)
            .expect("lru tail must be indexed");
        self.unlink(slot);
        let evicted = std::mem::replace(
            &mut self.slots[slot],
            Slot {
                key,
                value,
                prev: None,
                next: None,
            },
        );
        self.by_key.remove(evicted_pos);
        let pos = if pos > evicted_pos { pos - 1 } else { pos };
        self.by_key.insert(pos, slot);
        self.link_front(slot);
        Some(InsertReplaced::LruEvicted(evicted.key, evicted.value))
    }

    fn find(&self, key: &K) -> Result<usize, usize> {
        self.by_key
            .binary_search_by(|&slot| self.slots[slot].key.cmp(key))
    }

    fn touch(&mut self, slot: usize) {
        if self.head != Some(slot) {
            self.unlink(slot);
            self.link_front(slot);
        }
    }

    fn unlink(&mut self, slot: usize) {
        let Slot { prev, next, .. } = self.slots[slot];
        match prev {
            Some(prev) => self.slots[prev].next = next,
            None => self.head = next,
        }
        match next {
            Some(next) => self.slots[next].prev = prev,
            None => self.tail = prev,
        }
    }

    fn link_front(&mut self, slot: usize) {
        let old_head = self.head.replace(slot);
        match old_head {
            Some(old_head) => self.slots[old_head].prev = Some(slot),
            None => self.tail = Some(slot),
        }
        let node = &mut self.slots[slot];
        node.prev = None;
        node.next = old_head;
    }
}
