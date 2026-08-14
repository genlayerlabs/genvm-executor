use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use super::*;
use crate::rt::errors::{self, ResultExt};
use anyhow::Context;
use symbol_table::GlobalSymbol;

/// A shared, initialize-once cell holding an archive. A live [`ArchivePin`] keeps
/// it resident; the weak caches ([`WeakCache`]) that index it hold only weak
/// references to it.
pub type Cell = Arc<tokio::sync::OnceCell<ArchiveCache>>;

/// A strong reference (pin) to loaded runner content. While at least one pin is
/// held the content stays resident; when the last pin drops the weak map's entry
/// becomes dead and is purged on next access, freeing the bytes. Pins live in a
/// VM's [`LoadedSet`] (its store data) and thus die with the VM, when the bytes
/// become freeable.
#[derive(Clone)]
pub struct ArchivePin(Cell);

impl ArchivePin {
    fn entry(&self) -> &ArchiveCache {
        self.0
            .get()
            .expect("an ArchivePin always wraps an initialized cell")
    }

    /// The charged size of this runner's content -- the archive `total_size`,
    /// which for every source equals the raw content length.
    pub fn total_size(&self) -> u32 {
        self.entry().files.total_size
    }
}

impl std::ops::Deref for ArchivePin {
    type Target = ArchiveCache;

    fn deref(&self) -> &ArchiveCache {
        self.entry()
    }
}

impl std::fmt::Debug for ArchivePin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ArchivePin")
            .field(&self.runner_id().as_str())
            .finish()
    }
}

/// Per-VM **loaded set**: the single charge-dedup set and pin holder. Maps a
/// canonical runner id to the pin keeping its content resident. Stored next to
/// the VM's limiter (in its store data), **not** in the accumulator: the
/// accumulator round-trips through sandbox children, whereas the loaded set must
/// die with its VM. It is both the "already loaded -> free" set and the pin
/// holder; those must never be split.
#[derive(Default)]
pub struct LoadedSet(HashMap<GlobalSymbol, ArchivePin>);

impl LoadedSet {
    /// Whether `id` is already loaded in this VM (-> a load action is free).
    pub fn contains(&self, id: GlobalSymbol) -> bool {
        self.0.contains_key(&id)
    }

    pub fn get(&self, id: GlobalSymbol) -> Option<&ArchivePin> {
        self.0.get(&id)
    }

    /// Records a freshly charged pin. The load action guarantees the id was not
    /// already present (it returns early on a hit), so this always inserts.
    pub fn insert(&mut self, pin: ArchivePin) {
        let id = pin.runner_id();
        let prev = self.0.insert(id, pin);
        debug_assert!(
            prev.is_none(),
            "loaded set re-inserted an already-loaded runner {}",
            id.as_str(),
        );
        #[cfg(debug_assertions)]
        self.assert_invariants();
    }

    /// Pins of the `custom:` runners loaded in this VM, in id order -- the set a
    /// child may be granted. The string-prefix filter is sound
    /// because `parse_runner_id` reserves `custom:` (a builtin name can never be
    /// `custom`), so no other id kind can produce the prefix canonically.
    pub fn custom_pins(&self) -> Vec<ArchivePin> {
        let mut out: Vec<ArchivePin> = self
            .0
            .iter()
            .filter(|(id, _)| id.as_str().starts_with("custom:"))
            .map(|(_, pin)| pin.clone())
            .collect();
        out.sort_by_key(|pin| pin.runner_id().as_str());
        out
    }

    /// Every entry is keyed by its pin's canonical runner id.
    #[cfg(debug_assertions)]
    fn assert_invariants(&self) {
        for (key, pin) in &self.0 {
            debug_assert!(
                pin.runner_id() == *key,
                "loaded set keyed {} holds a pin for {}",
                key.as_str(),
                pin.runner_id().as_str(),
            );
        }
    }
}

/// Weakly-held content index: the map owns only [`Weak`](std::sync::Weak)
/// references, so an entry lives exactly as long as some [`ArchivePin`] to it
/// does. Dead weaks are purged lazily when their key is next touched. Backs both
/// the disk/chain archive cache and the custom-runner registry -- a pure dedup
/// mechanism whose state a VM cannot observe.
pub struct WeakCache(
    dashmap::DashMap<GlobalSymbol, std::sync::Weak<tokio::sync::OnceCell<ArchiveCache>>>,
);

impl Default for WeakCache {
    fn default() -> Self {
        Self::new()
    }
}

impl WeakCache {
    pub fn new() -> Self {
        Self(dashmap::DashMap::new())
    }

    /// Returns a strong cell for `key`, deduplicating concurrent creation: two
    /// VMs racing on the same id observe the same cell (so a single creator
    /// runs), while a dead weak is replaced with a fresh cell (purge).
    pub fn cell(&self, key: GlobalSymbol) -> Cell {
        // The entry guard must drop before `assert_invariants` iterates the map:
        // `iter()` locks every shard, `entry()` holds one, so overlapping them on
        // the same thread would deadlock.
        let strong = match self.0.entry(key) {
            dashmap::Entry::Occupied(mut occ) => match occ.get().upgrade() {
                Some(strong) => strong,
                None => {
                    let strong = Arc::new(tokio::sync::OnceCell::new());
                    occ.insert(Arc::downgrade(&strong));
                    strong
                }
            },
            dashmap::Entry::Vacant(vac) => {
                let strong = Arc::new(tokio::sync::OnceCell::new());
                vac.insert(Arc::downgrade(&strong));
                strong
            }
        };
        #[cfg(debug_assertions)]
        self.assert_invariants();
        strong
    }

    fn insert_strong(&self, key: GlobalSymbol, cell: &Cell) {
        self.0.insert(key, Arc::downgrade(cell));
        #[cfg(debug_assertions)]
        self.assert_invariants();
    }

    /// Every live weak upgrades to a cell that, once its `OnceCell` is
    /// initialized, holds an [`ArchiveCache`] whose id equals the map key. A
    /// dead weak or a not-yet-initialized cell is vacuously fine. O(entries),
    /// debug-only.
    #[cfg(debug_assertions)]
    fn assert_invariants(&self) {
        for entry in self.0.iter() {
            let Some(strong) = entry.value().upgrade() else {
                continue;
            };
            let Some(cache) = strong.get() else {
                continue;
            };
            debug_assert!(
                cache.runner_id() == *entry.key(),
                "cache entry keyed {} holds an archive with id {}",
                entry.key().as_str(),
                cache.runner_id().as_str(),
            );
        }
    }

    /// Teardown check: no live entry may remain. Called after every VM (and thus
    /// every pin) is dropped, asserting the invariant that resident
    /// content is always covered by a live pin -- with no live VMs, none is
    /// pinned. Debug-only.
    #[cfg(debug_assertions)]
    pub fn assert_empty_on_teardown(&self, what: &str) {
        for entry in self.0.iter() {
            debug_assert!(
                entry.value().upgrade().is_none(),
                "{what} {} still pinned at teardown",
                entry.key().as_str()
            );
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct StrSymbol(symbol_table::GlobalSymbol);

impl StrSymbol {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::borrow::Borrow<str> for StrSymbol {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

impl AsRef<str> for StrSymbol {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl Ord for StrSymbol {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl PartialOrd for StrSymbol {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl serde::Serialize for StrSymbol {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_str().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for StrSymbol {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(Self(symbol_table::GlobalSymbol::from(s)))
    }
}

pub struct Reader {
    cache: WeakCache,
    runners_data_path: std::path::PathBuf,

    all: BTreeMap<StrSymbol, Vec<StrSymbol>>,
    latest: BTreeMap<StrSymbol, StrSymbol>,
}

impl Drop for Reader {
    fn drop(&mut self) {
        // Supervisor teardown happens after every VM is dropped, so every pin is
        // gone and the weak map must hold no live entry.
        #[cfg(debug_assertions)]
        self.cache.assert_empty_on_teardown("archive");
    }
}

pub fn validate_builtin_runner_hash(hash: &str) -> anyhow::Result<()> {
    if hash.len() != 52 {
        anyhow::bail!(
            "runner hash must be 32 characters long, it is {} for `{hash}`",
            hash.len()
        );
    }

    for c in hash.chars() {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() {
            anyhow::bail!("runner hash must be lowercase alphanumeric, found {c:?}");
        }
    }

    Ok(())
}

pub fn validate_builtin_runner_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("runner name cannot be empty");
    }
    if name == "custom" {
        anyhow::bail!("runner name cannot be `custom`");
    }
    for c in name.chars() {
        if !c.is_ascii_alphanumeric() && !['-', '_', '.'].contains(&c) {
            anyhow::bail!("runner name must be alphanumeric or \"._-\", found {c:?}");
        }
    }

    if name.chars().all(|c| ['-', '_', '.'].contains(&c)) {
        anyhow::bail!("runner name cannot consist solely of \"._-\"");
    }

    Ok(())
}

pub trait RegistryPayload: serde::de::DeserializeOwned {
    fn validate(&self) -> anyhow::Result<()>;
}

impl RegistryPayload for String {
    fn validate(&self) -> anyhow::Result<()> {
        validate_builtin_runner_hash(self.as_ref())
    }
}

impl RegistryPayload for StrSymbol {
    fn validate(&self) -> anyhow::Result<()> {
        validate_builtin_runner_hash(self.as_ref())
    }
}

impl<T: RegistryPayload> RegistryPayload for Vec<T> {
    fn validate(&self) -> anyhow::Result<()> {
        for item in self {
            item.validate()?;
        }
        Ok(())
    }
}

pub fn read_registry<K: Ord + serde::de::DeserializeOwned + AsRef<str>, V: RegistryPayload>(
    path: &std::path::Path,
) -> anyhow::Result<BTreeMap<K, V>> {
    let res: BTreeMap<K, V> = serde_json::from_reader(
        std::fs::File::open(path).with_ctx(|| format!("opening {path:?}"))?,
    )?;

    (|| {
        for (k, v) in &res {
            validate_builtin_runner_name(k.as_ref())?;
            v.validate()?;
        }
        Ok::<(), anyhow::Error>(())
    })()
    .with_context(|| format!("validating {path:?}"))?;

    Ok(res)
}

impl Reader {
    pub fn new(
        path: &std::path::Path,
        registry_path: &std::path::Path,
        debug_mode: bool,
    ) -> errors::Result<Self> {
        let runners_path = std::path::PathBuf::from(path);
        if !runners_path.exists() {
            return Err(errors::Error::internal(format!(
                "path {:#?} doesn't exist",
                &runners_path
            )));
        }

        let mut all: BTreeMap<_, Vec<_>> = read_registry(&registry_path.join("all.json"))?;
        for b in all.values_mut() {
            b.sort();
        }

        let latest = if debug_mode {
            read_registry(&registry_path.join("latest.json"))?
        } else {
            BTreeMap::new()
        };

        Ok(Self {
            cache: WeakCache::new(),
            runners_data_path: runners_path.clone(),
            all,
            latest,
        })
    }

    pub fn get_latest(&self, id: &str) -> Option<&str> {
        self.latest.get(id).map(|s| s.as_str())
    }

    pub fn has_in_all(&self, id: &str, hash: &str) -> bool {
        match self.all.get(id) {
            Some(hashes) => hashes.binary_search_by(|h| h.as_str().cmp(hash)).is_ok(),
            None => false,
        }
    }

    pub fn runners_path(&self) -> &std::path::Path {
        &self.runners_data_path
    }

    /// The shared, weakly-held content cell for a disk/`chain:` runner id. The
    /// load action attaches to it (if a live pin already initialized it) or
    /// initializes it via its own creator.
    pub fn cell(&self, id: symbol_table::GlobalSymbol) -> Cell {
        self.cache.cell(id)
    }

    /// Inserts `archive` under `id` and returns a pin the caller must hold for
    /// as long as the entry should stay resident. Used by the deploy
    /// prepopulation path, whose pin is held by the top-level execution scope
    /// until the root VM's main load action attaches to (and charges for) it.
    pub fn put(&self, id: symbol_table::GlobalSymbol, archive: Archive) -> ArchivePin {
        let cell = Arc::new(tokio::sync::OnceCell::new_with(Some(ArchiveCache::new(
            id, archive,
        ))));
        self.cache.insert_strong(id, &cell);
        let pin = ArchivePin(cell);
        // The cell is initialized (deref would panic otherwise) and keyed by `id`.
        #[cfg(debug_assertions)]
        debug_assert!(
            pin.runner_id() == id,
            "put({}) produced a pin for {}",
            id.as_str(),
            pin.runner_id().as_str(),
        );
        pin
    }
}

/// Wraps a shared content cell into a pin. The load action uses this after
/// initializing (or attaching to) a cell obtained from a [`WeakCache`].
pub(crate) fn pin_of(cell: Cell) -> ArchivePin {
    // Catch misuse here rather than at a later, harder-to-attribute deref panic.
    debug_assert!(
        cell.initialized(),
        "pin_of called with an uninitialized cell"
    );
    ArchivePin(cell)
}

pub fn get_cache_dir(base_path: &str) -> errors::Result<std::path::PathBuf> {
    let base_path = std::path::Path::new(base_path);

    std::fs::create_dir_all(base_path).with_ctx(|| "creating cache dir".to_string())?;

    let test_path = base_path.join(".test");
    std::fs::write(test_path, "").with_ctx(|| "creating test file".to_string())?;
    Ok(base_path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(s: &str) -> GlobalSymbol {
        GlobalSymbol::from(s)
    }

    fn archive_of(size: u32) -> Archive {
        Archive {
            data: BTreeMap::new(),
            total_size: size,
        }
    }

    fn cache_with(id: GlobalSymbol, size: u32) -> (WeakCache, ArchivePin) {
        let cache = WeakCache::new();
        let cell = cache.cell(id);
        cell.set(ArchiveCache::new(id, archive_of(size)))
            .unwrap_or_else(|_| panic!("fresh cell"));
        (cache, ArchivePin(cell))
    }

    // -- LoadedSet -------------------------------------------------------

    #[test]
    fn loaded_set_reports_membership_and_pin() {
        let (_c, pin) = cache_with(sym("py:abc"), 10);
        let mut set = LoadedSet::default();
        assert!(!set.contains(sym("py:abc")));
        set.insert(pin);
        assert!(set.contains(sym("py:abc")));
        assert_eq!(set.get(sym("py:abc")).unwrap().total_size(), 10);
    }

    #[test]
    fn custom_pins_returns_only_custom_entries_sorted() {
        let mut set = LoadedSet::default();
        let (_c1, p1) = cache_with(sym("custom:bbb"), 1);
        let (_c2, p2) = cache_with(sym("py:abc"), 2);
        let (_c3, p3) = cache_with(sym("custom:aaa"), 3);
        set.insert(p1);
        set.insert(p2);
        set.insert(p3);
        let ids: Vec<&str> = set
            .custom_pins()
            .iter()
            .map(|p| p.runner_id().as_str())
            .collect();
        assert_eq!(ids, vec!["custom:aaa", "custom:bbb"], "only custom, sorted");
    }

    // -- WeakCache eviction ----------------------------------------------

    #[test]
    fn weak_cache_evicts_when_last_pin_drops() {
        let cache = WeakCache::new();
        let id = sym("py:abc");

        let cell = cache.cell(id);
        cell.set(ArchiveCache::new(id, archive_of(10)))
            .unwrap_or_else(|_| panic!("fresh cell"));
        let pin = ArchivePin(cell);
        // A second lookup while pinned observes the same, still-initialized cell.
        assert!(cache.cell(id).initialized(), "pinned entry stays resident");

        drop(pin);
        // With no pin the weak is dead; the next lookup yields a fresh, empty cell.
        assert!(
            !cache.cell(id).initialized(),
            "entry evicted after last pin drop"
        );
    }
}
