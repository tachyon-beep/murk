//! Field handles and location descriptors.
//!
//! A [`FieldHandle`] encodes the physical location of a field's data within
//! the arena. It is generation-scoped: the `generation` field allows O(1)
//! staleness checks without a lookup table.

use std::fmt;

/// Physical location of a field allocation within the arena.
///
/// Handles are internal to `murk-arena` and never cross the FFI boundary.
/// They encode enough information to resolve a `&[f32]` slice in O(1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub struct FieldHandle {
    /// Arena generation when this allocation was made.
    pub(crate) generation: u32,
    /// Byte offset within the target segment.
    pub(crate) offset: u32,
    /// Length of the allocation in f32 elements.
    pub(crate) len: u32,
    /// Which pool and segment this handle points into.
    pub(crate) location: FieldLocation,
}

impl FieldHandle {
    /// Create a new handle.
    pub(crate) fn new(generation: u32, offset: u32, len: u32, location: FieldLocation) -> Self {
        Self {
            generation,
            offset,
            len,
            location,
        }
    }

    /// The generation this handle belongs to.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Length of the allocation in f32 elements.
    pub fn len(&self) -> u32 {
        self.len
    }

    /// Whether this is a zero-length allocation.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The location descriptor.
    pub fn location(&self) -> FieldLocation {
        self.location
    }
}

impl fmt::Display for FieldHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FieldHandle(gen={}, off={}, len={}, {:?})",
            self.generation, self.offset, self.len, self.location
        )
    }
}

/// Describes which segment pool a [`FieldHandle`] points into.
///
/// The arena maintains three separate segment pools plus a static arena.
/// This enum tells the resolve path which pool to look up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldLocation {
    /// In the current per-tick buffer (alternating A/B on ping-pong).
    PerTick {
        /// Index into the per-tick segment list.
        segment_index: u16,
    },
    /// In the dedicated sparse segment pool.
    Sparse {
        /// Index into the sparse segment list.
        segment_index: u16,
    },
    /// In the static arena (generation 0 forever, `Arc`-shared).
    Static {
        /// Byte offset within the static arena's data vec.
        offset: u32,
        /// Length in f32 elements.
        len: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_round_trip() {
        let loc = FieldLocation::PerTick { segment_index: 3 };
        let h = FieldHandle::new(42, 1024, 256, loc);
        assert_eq!(h.generation(), 42);
        assert_eq!(h.len(), 256);
        assert!(!h.is_empty());
        assert_eq!(h.location(), loc);
    }

    #[test]
    fn empty_handle() {
        let h = FieldHandle::new(0, 0, 0, FieldLocation::PerTick { segment_index: 0 });
        assert!(h.is_empty());
    }

    #[test]
    fn static_location_stores_offset_and_len() {
        let loc = FieldLocation::Static {
            offset: 100,
            len: 50,
        };
        if let FieldLocation::Static { offset, len } = loc {
            assert_eq!(offset, 100);
            assert_eq!(len, 50);
        } else {
            panic!("expected Static");
        }
    }
}
