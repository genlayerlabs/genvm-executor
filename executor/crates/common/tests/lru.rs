use genvm_common::util::lru::{InsertReplaced, Lru};

fn filled<const CAP: usize>(keys: &[u32]) -> Lru<u32, u32, CAP> {
    let mut lru = Lru::new();
    for &k in keys {
        assert_eq!(lru.insert(k, k * 10), None);
    }
    lru
}

#[test]
fn get_returns_inserted_value() {
    let mut lru = filled::<4>(&[3, 1, 2]);
    assert_eq!(lru.get(&1), Some(&10));
    assert_eq!(lru.get(&2), Some(&20));
    assert_eq!(lru.get(&3), Some(&30));
    assert_eq!(lru.get(&4), None);
}

#[test]
fn insert_existing_key_returns_old_value() {
    let mut lru = filled::<2>(&[1, 2]);
    assert_eq!(lru.insert(1, 11), Some(InsertReplaced::OldValue(10)));
    assert_eq!(lru.get(&1), Some(&11));
}

#[test]
fn full_insert_evicts_least_recently_inserted() {
    let mut lru = filled::<3>(&[1, 2, 3]);
    assert_eq!(lru.insert(4, 40), Some(InsertReplaced::LruEvicted(1, 10)));
    assert_eq!(lru.insert(5, 50), Some(InsertReplaced::LruEvicted(2, 20)));
    assert_eq!(lru.get(&1), None);
    assert_eq!(lru.get(&3), Some(&30));
}

#[test]
fn get_protects_entry_from_eviction() {
    let mut lru = filled::<3>(&[1, 2, 3]);
    lru.get(&1);
    assert_eq!(lru.insert(4, 40), Some(InsertReplaced::LruEvicted(2, 20)));
}

#[test]
fn get_miss_does_not_change_order() {
    let mut lru = filled::<2>(&[1, 2]);
    assert_eq!(lru.get(&7), None);
    assert_eq!(lru.insert(3, 30), Some(InsertReplaced::LruEvicted(1, 10)));
}

#[test]
fn overwrite_protects_entry_from_eviction() {
    let mut lru = filled::<3>(&[1, 2, 3]);
    lru.insert(1, 11);
    assert_eq!(lru.insert(4, 40), Some(InsertReplaced::LruEvicted(2, 20)));
}

#[test]
fn eviction_keeps_keys_findable_on_both_sides() {
    // the new key sorts before and after the evicted one in turn
    let mut lru = filled::<3>(&[20, 10, 30]);
    assert_eq!(lru.insert(5, 50), Some(InsertReplaced::LruEvicted(20, 200)));
    assert_eq!(
        lru.insert(40, 400),
        Some(InsertReplaced::LruEvicted(10, 100))
    );
    for (k, v) in [(5, 50), (30, 300), (40, 400)] {
        assert_eq!(lru.get(&k), Some(&v), "key {k}");
    }
    assert_eq!(lru.get(&10), None);
    assert_eq!(lru.get(&20), None);
}

#[test]
fn eviction_into_the_evicted_keys_sort_position() {
    let mut lru = filled::<3>(&[20, 10, 30]);
    assert_eq!(
        lru.insert(15, 150),
        Some(InsertReplaced::LruEvicted(20, 200))
    );
    assert_eq!(
        lru.insert(25, 250),
        Some(InsertReplaced::LruEvicted(10, 100))
    );
    for (k, v) in [(15, 150), (25, 250), (30, 300)] {
        assert_eq!(lru.get(&k), Some(&v), "key {k}");
    }
}

#[test]
fn single_slot_always_evicts_previous() {
    let mut lru = filled::<1>(&[1]);
    assert_eq!(lru.insert(2, 20), Some(InsertReplaced::LruEvicted(1, 10)));
    assert_eq!(lru.insert(2, 21), Some(InsertReplaced::OldValue(20)));
    assert_eq!(lru.get(&2), Some(&21));
}

#[test]
fn zero_capacity_stores_nothing() {
    let mut lru = Lru::<u32, u32, 0>::new();
    assert_eq!(lru.insert(1, 10), None);
    assert_eq!(lru.get(&1), None);
}

#[test]
fn clone_is_independent_and_keeps_order() {
    let mut lru = filled::<2>(&[1, 2]);
    let mut copy = lru.clone();
    lru.get(&1);
    assert_eq!(copy.insert(3, 30), Some(InsertReplaced::LruEvicted(1, 10)));
    assert_eq!(lru.insert(3, 30), Some(InsertReplaced::LruEvicted(2, 20)));
}

#[test]
fn long_mixed_sequence_matches_reference_model() {
    const CAP: usize = 8;
    let mut lru = Lru::<u32, u32, CAP>::new();
    // most recently used last
    let mut model: Vec<(u32, u32)> = Vec::new();
    let mut seed: u32 = 1;
    for step in 0..10_000u32 {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let key = (seed >> 16) % 20;
        if (seed >> 24) & 1 == 0 {
            let expected = model.iter().position(|&(k, _)| k == key).map(|i| {
                let entry = model.remove(i);
                model.push(entry);
                entry.1
            });
            assert_eq!(lru.get(&key).copied(), expected, "step {step}");
        } else {
            let expected = if let Some(i) = model.iter().position(|&(k, _)| k == key) {
                let (_, old) = model.remove(i);
                Some(InsertReplaced::OldValue(old))
            } else if model.len() == CAP {
                let (k, v) = model.remove(0);
                Some(InsertReplaced::LruEvicted(k, v))
            } else {
                None
            };
            model.push((key, step));
            assert_eq!(lru.insert(key, step), expected, "step {step}");
        }
    }
}
