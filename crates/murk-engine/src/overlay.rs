//! Overlay field caches for tick execution.
//!
//! The tick engine needs two kinds of cached field data:
//!
//! - **Base generation fields** — copied from the arena snapshot before
//!   `begin_tick()`, because `snapshot()` borrows `&self` while
//!   `begin_tick()` borrows `&mut self`.
//!
//! - **Staged fields** — copied from `guard.writer.read()` between
//!   propagators, so the `&self` read borrow is released before
//!   constructing `StepContext` with `&mut guard.writer`.
//!
//! [`OverlayReader`] routes each `(propagator, field)` read to the
//! appropriate cache based on the [`ReadResolutionPlan`].

use indexmap::IndexMap;
use murk_core::id::FieldId;
use murk_core::traits::FieldReader;
use murk_propagator::pipeline::ReadSource;

// ── BaseFieldSet ─────────────────────────────────────────────────

/// Set of fields to pre-copy from the base snapshot each tick.
///
/// Computed once at startup from the [`ReadResolutionPlan`]: the union of
/// all `BaseGen`-routed reads plus all `reads_previous` fields across
/// every propagator.
pub(crate) struct BaseFieldSet {
    field_ids: Vec<FieldId>,
}

impl BaseFieldSet {
    /// Build from the plan and propagators' `reads_previous()` declarations.
    pub(crate) fn from_plan(
        plan: &murk_propagator::ReadResolutionPlan,
        propagators: &[Box<dyn murk_propagator::Propagator>],
    ) -> Self {
        let mut set = indexmap::IndexSet::new();

        // All BaseGen-routed reads.
        for i in 0..plan.len() {
            if let Some(routes) = plan.routes_for(i) {
                for (&field, &source) in routes {
                    if source == ReadSource::BaseGen {
                        set.insert(field);
                    }
                }
            }
        }

        // All reads_previous fields.
        for prop in propagators {
            for field in prop.reads_previous().iter() {
                set.insert(field);
            }
        }

        // All WriteMode::Incremental fields (need previous-gen data to seed
        // write buffers before step()).
        for i in 0..plan.len() {
            for field in plan.incremental_fields_for(i) {
                set.insert(field);
            }
        }

        Self {
            field_ids: set.into_iter().collect(),
        }
    }

    /// The field IDs that must be copied from the base snapshot.
    pub(crate) fn field_ids(&self) -> &[FieldId] {
        &self.field_ids
    }
}

// ── BaseFieldCache ───────────────────────────────────────────────

/// Standalone [`FieldReader`] holding copied base-generation field data.
///
/// Populated once per tick before `begin_tick()` by reading from
/// [`Snapshot`](murk_arena::Snapshot).
///
/// Uses `Option<Vec<f32>>` to distinguish unpopulated/stale fields (`None`)
/// from populated fields that happen to be empty (`Some(vec![])`).
pub(crate) struct BaseFieldCache {
    entries: IndexMap<FieldId, Option<Vec<f32>>>,
}

impl BaseFieldCache {
    /// Create an empty cache.
    pub(crate) fn new() -> Self {
        Self {
            entries: IndexMap::new(),
        }
    }

    /// Populate the cache from a snapshot for the given field set.
    ///
    /// Any field not found in the snapshot is silently skipped (it may be
    /// a PerTick field that hasn't been written yet on the very first tick).
    pub(crate) fn populate(&mut self, snapshot: &dyn FieldReader, fields: &BaseFieldSet) {
        // Mark all entries stale.
        for v in self.entries.values_mut() {
            *v = None;
        }

        for &field in fields.field_ids() {
            if let Some(data) = snapshot.read(field) {
                let slot = self.entries.entry(field).or_insert(None);
                let buf = slot.get_or_insert_with(|| Vec::with_capacity(data.len()));
                buf.clear();
                buf.extend_from_slice(data);
            }
        }
    }
}

impl FieldReader for BaseFieldCache {
    fn read(&self, field: FieldId) -> Option<&[f32]> {
        self.entries.get(&field).and_then(|v| v.as_deref())
    }
}

// ── StagedFieldCache ─────────────────────────────────────────────

/// Cached copies of staged fields for a single propagator's overlay reads.
///
/// Cleared and refilled between propagators.
///
/// Uses `Option<Vec<f32>>` to distinguish stale/cleared fields (`None`)
/// from populated fields that happen to be empty (`Some(vec![])`).
pub(crate) struct StagedFieldCache {
    entries: IndexMap<FieldId, Option<Vec<f32>>>,
}

impl StagedFieldCache {
    /// Create an empty cache.
    pub(crate) fn new() -> Self {
        Self {
            entries: IndexMap::new(),
        }
    }

    /// Clear all entries (marks as stale; keeps map keys).
    pub(crate) fn clear(&mut self) {
        for v in self.entries.values_mut() {
            *v = None;
        }
    }

    /// Insert (or replace) a field's data.
    pub(crate) fn insert(&mut self, field: FieldId, data: &[f32]) {
        let slot = self.entries.entry(field).or_insert(None);
        let buf = slot.get_or_insert_with(|| Vec::with_capacity(data.len()));
        buf.clear();
        buf.extend_from_slice(data);
    }
}

impl FieldReader for StagedFieldCache {
    fn read(&self, field: FieldId) -> Option<&[f32]> {
        self.entries.get(&field).and_then(|v| v.as_deref())
    }
}

// ── OverlayReader ────────────────────────────────────────────────

/// Per-propagator [`FieldReader`] routing reads per [`ReadResolutionPlan`].
///
/// - `BaseGen` reads → `BaseFieldCache`
/// - `Staged` reads → `StagedFieldCache`
/// - Unknown fields → `None`
pub(crate) struct OverlayReader<'a> {
    routes: &'a IndexMap<FieldId, ReadSource>,
    base_cache: &'a BaseFieldCache,
    staged_cache: &'a StagedFieldCache,
}

impl<'a> OverlayReader<'a> {
    pub(crate) fn new(
        routes: &'a IndexMap<FieldId, ReadSource>,
        base_cache: &'a BaseFieldCache,
        staged_cache: &'a StagedFieldCache,
    ) -> Self {
        Self {
            routes,
            base_cache,
            staged_cache,
        }
    }
}

impl FieldReader for OverlayReader<'_> {
    fn read(&self, field: FieldId) -> Option<&[f32]> {
        match self.routes.get(&field)? {
            ReadSource::BaseGen => self.base_cache.read(field),
            ReadSource::Staged { .. } => self.staged_cache.read(field),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_cache_with(fields: &[(FieldId, Vec<f32>)]) -> BaseFieldCache {
        let mut cache = BaseFieldCache::new();
        for (id, data) in fields {
            cache.entries.insert(*id, Some(data.clone()));
        }
        cache
    }

    fn staged_cache_with(fields: &[(FieldId, Vec<f32>)]) -> StagedFieldCache {
        let mut cache = StagedFieldCache::new();
        for (id, data) in fields {
            cache.insert(*id, data);
        }
        cache
    }

    #[test]
    fn base_gen_routing_delegates_to_base_cache() {
        let base = base_cache_with(&[(FieldId(0), vec![1.0, 2.0, 3.0])]);
        let staged = StagedFieldCache::new();
        let mut routes = IndexMap::new();
        routes.insert(FieldId(0), ReadSource::BaseGen);

        let reader = OverlayReader::new(&routes, &base, &staged);
        assert_eq!(reader.read(FieldId(0)), Some(&[1.0, 2.0, 3.0][..]));
    }

    #[test]
    fn staged_routing_delegates_to_staged_cache() {
        let base = BaseFieldCache::new();
        let staged = staged_cache_with(&[(FieldId(1), vec![10.0, 20.0])]);
        let mut routes = IndexMap::new();
        routes.insert(FieldId(1), ReadSource::Staged { writer_index: 0 });

        let reader = OverlayReader::new(&routes, &base, &staged);
        assert_eq!(reader.read(FieldId(1)), Some(&[10.0, 20.0][..]));
    }

    #[test]
    fn mixed_base_and_staged_routing() {
        let base = base_cache_with(&[(FieldId(0), vec![1.0])]);
        let staged = staged_cache_with(&[(FieldId(1), vec![99.0])]);
        let mut routes = IndexMap::new();
        routes.insert(FieldId(0), ReadSource::BaseGen);
        routes.insert(FieldId(1), ReadSource::Staged { writer_index: 0 });

        let reader = OverlayReader::new(&routes, &base, &staged);
        assert_eq!(reader.read(FieldId(0)), Some(&[1.0][..]));
        assert_eq!(reader.read(FieldId(1)), Some(&[99.0][..]));
    }

    #[test]
    fn unknown_field_returns_none() {
        let base = BaseFieldCache::new();
        let staged = StagedFieldCache::new();
        let routes = IndexMap::new();

        let reader = OverlayReader::new(&routes, &base, &staged);
        assert_eq!(reader.read(FieldId(42)), None);
    }

    #[test]
    fn staged_cache_clear_and_refill() {
        let mut cache = StagedFieldCache::new();
        cache.insert(FieldId(0), &[1.0, 2.0]);
        assert_eq!(cache.read(FieldId(0)), Some(&[1.0, 2.0][..]));

        cache.clear();
        assert_eq!(cache.read(FieldId(0)), None);

        cache.insert(FieldId(0), &[3.0, 4.0]);
        assert_eq!(cache.read(FieldId(0)), Some(&[3.0, 4.0][..]));
    }

    // ── BUG-008 regression tests ─────────────────────────────────

    #[test]
    fn base_cache_empty_field_returns_some_empty_slice() {
        // A zero-component field (e.g., FieldType::Vector { dims: 0 })
        // must return Some(&[]) not None.
        let base = base_cache_with(&[(FieldId(5), vec![])]);
        assert_eq!(base.read(FieldId(5)), Some(&[][..]));
    }

    #[test]
    fn staged_cache_empty_field_returns_some_empty_slice() {
        // A zero-component field inserted into the staged cache
        // must return Some(&[]) not None.
        let mut cache = StagedFieldCache::new();
        cache.insert(FieldId(7), &[]);
        assert_eq!(cache.read(FieldId(7)), Some(&[][..]));
    }

    #[test]
    fn staged_cache_clear_makes_empty_field_stale() {
        // After clear(), even a previously-populated empty field must
        // return None (stale), not Some(&[]).
        let mut cache = StagedFieldCache::new();
        cache.insert(FieldId(7), &[]);
        assert_eq!(cache.read(FieldId(7)), Some(&[][..]));

        cache.clear();
        assert_eq!(cache.read(FieldId(7)), None);
    }

    #[test]
    fn overlay_routes_empty_base_field() {
        let base = base_cache_with(&[(FieldId(0), vec![])]);
        let staged = StagedFieldCache::new();
        let mut routes = IndexMap::new();
        routes.insert(FieldId(0), ReadSource::BaseGen);

        let reader = OverlayReader::new(&routes, &base, &staged);
        assert_eq!(reader.read(FieldId(0)), Some(&[][..]));
    }

    #[test]
    fn overlay_routes_empty_staged_field() {
        let base = BaseFieldCache::new();
        let staged = staged_cache_with(&[(FieldId(1), vec![])]);
        let mut routes = IndexMap::new();
        routes.insert(FieldId(1), ReadSource::Staged { writer_index: 0 });

        let reader = OverlayReader::new(&routes, &base, &staged);
        assert_eq!(reader.read(FieldId(1)), Some(&[][..]));
    }
}
