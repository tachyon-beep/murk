//! Contiguous memory segments and growable segment lists.
//!
//! A [`Segment`] is a 64MB (default) contiguous `Vec<f32>` with bump allocation.
//! A [`SegmentList`] is a growable collection of segments that overflow into
//! new segments when the current one is full.

use crate::error::ArenaError;

/// A single contiguous memory segment with bump allocation.
///
/// Segments are the fundamental storage unit of the arena. Each segment is
/// a pre-allocated `Vec<f32>` with a cursor that advances on each allocation.
/// Segments are never freed during runtime — only reset or dropped at shutdown.
pub struct Segment {
    /// Backing storage. Allocated to full capacity at creation.
    data: Vec<f32>,
    /// Bump pointer: next free position (in f32 elements).
    cursor: usize,
}

impl Segment {
    /// Create a new segment with the given capacity (in f32 elements).
    ///
    /// The segment is zero-initialised (Phase 1 safety guarantee).
    pub fn new(capacity: u32) -> Self {
        Self {
            data: vec![0.0; capacity as usize],
            cursor: 0,
        }
    }

    /// Bump-allocate `len` f32 elements from this segment.
    ///
    /// Returns `Some((offset, &mut [f32]))` where `offset` is the starting
    /// position within this segment, or `None` if there is insufficient
    /// remaining capacity.
    pub fn alloc(&mut self, len: u32) -> Option<(u32, &mut [f32])> {
        let len = len as usize;
        let new_cursor = self.cursor.checked_add(len)?;
        if new_cursor > self.data.len() {
            return None;
        }
        let offset = self.cursor as u32;
        let slice = &mut self.data[self.cursor..new_cursor];
        self.cursor = new_cursor;
        // Zero-init the allocated region (Phase 1).
        slice.fill(0.0);
        Some((offset, slice))
    }

    /// Get a shared slice at the given offset and length.
    ///
    /// # Panics
    ///
    /// Panics if `offset + len` exceeds the segment's allocated region.
    pub fn slice(&self, offset: u32, len: u32) -> &[f32] {
        let start = offset as usize;
        let end = start + len as usize;
        &self.data[start..end]
    }

    /// Get a mutable slice at the given offset and length.
    ///
    /// # Panics
    ///
    /// Panics if `offset + len` exceeds the segment's allocated region.
    pub fn slice_mut(&mut self, offset: u32, len: u32) -> &mut [f32] {
        let start = offset as usize;
        let end = start + len as usize;
        &mut self.data[start..end]
    }

    /// Reset the bump pointer to zero without deallocating.
    ///
    /// All previous allocations become invalid. The backing memory is
    /// NOT zeroed — callers must zero on the next `alloc()` (which
    /// Phase 1 does automatically).
    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    /// Number of f32 elements currently allocated.
    pub fn used(&self) -> usize {
        self.cursor
    }

    /// Total capacity in f32 elements.
    pub fn capacity(&self) -> usize {
        self.data.len()
    }

    /// Remaining free capacity in f32 elements.
    pub fn remaining(&self) -> usize {
        self.data.len() - self.cursor
    }

    /// Memory usage of the backing storage in bytes.
    pub fn memory_bytes(&self) -> usize {
        self.data.len() * std::mem::size_of::<f32>()
    }
}

/// A growable list of [`Segment`]s with overflow-based bump allocation.
///
/// When the current segment is full, a new segment is appended (up to
/// `max_segments`). Allocations that span segment boundaries are placed
/// entirely in the next segment — there is no cross-segment splitting.
pub struct SegmentList {
    segments: Vec<Segment>,
    segment_size: u32,
    max_segments: u16,
    /// Index of the segment currently being filled.
    current: usize,
}

impl SegmentList {
    /// Create a new segment list with one pre-allocated segment.
    pub fn new(segment_size: u32, max_segments: u16) -> Self {
        let mut segments = Vec::with_capacity(max_segments as usize);
        segments.push(Segment::new(segment_size));
        Self {
            segments,
            segment_size,
            max_segments,
            current: 0,
        }
    }

    /// Bump-allocate `len` f32 elements, growing into a new segment if needed.
    ///
    /// Returns `Ok((segment_index, offset))` on success, or
    /// `Err(ArenaError::CapacityExceeded)` if `max_segments` would be exceeded.
    pub fn alloc(&mut self, len: u32) -> Result<(u16, u32), ArenaError> {
        // Reject allocations that can never fit in a single segment.
        if len > self.segment_size {
            return Err(ArenaError::CapacityExceeded {
                requested: len as usize * std::mem::size_of::<f32>(),
                capacity: self.segment_size as usize * std::mem::size_of::<f32>(),
            });
        }

        // Try the current segment first.
        if let Some((offset, _slice)) = self.segments[self.current].alloc(len) {
            return Ok((self.current as u16, offset));
        }

        // Current segment full — advance to the next existing segment or create one.
        let next = self.current + 1;
        if next < self.segments.len() {
            // Reuse existing segment (was allocated in a prior generation).
            if let Some((offset, _slice)) = self.segments[next].alloc(len) {
                self.current = next;
                return Ok((next as u16, offset));
            }
        }

        // Need a new segment.
        if self.segments.len() >= self.max_segments as usize {
            return Err(ArenaError::CapacityExceeded {
                requested: len as usize * std::mem::size_of::<f32>(),
                capacity: self.total_capacity_bytes(),
            });
        }

        let mut seg = Segment::new(self.segment_size);
        // len <= segment_size is guaranteed by the check above.
        let (offset, _slice) = seg
            .alloc(len)
            .expect("len <= segment_size, so fresh segment always fits");
        self.segments.push(seg);
        self.current = self.segments.len() - 1;
        Ok((self.current as u16, offset))
    }

    /// Get a shared slice from the given segment at the given offset and length.
    pub fn slice(&self, segment_index: u16, offset: u32, len: u32) -> &[f32] {
        self.segments[segment_index as usize].slice(offset, len)
    }

    /// Get a mutable slice from the given segment at the given offset and length.
    pub fn slice_mut(&mut self, segment_index: u16, offset: u32, len: u32) -> &mut [f32] {
        self.segments[segment_index as usize].slice_mut(offset, len)
    }

    /// Reset all segments' bump pointers without deallocating.
    ///
    /// After reset, allocations start from segment 0 again.
    pub fn reset(&mut self) {
        for seg in &mut self.segments {
            seg.reset();
        }
        self.current = 0;
    }

    /// Total number of segments currently allocated.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Total memory usage across all segments in bytes.
    pub fn memory_bytes(&self) -> usize {
        self.segments.iter().map(|s| s.memory_bytes()).sum()
    }

    /// Total used f32 elements across all segments.
    pub fn total_used(&self) -> usize {
        self.segments.iter().map(|s| s.used()).sum()
    }

    fn total_capacity_bytes(&self) -> usize {
        self.segments.len() * self.segment_size as usize * std::mem::size_of::<f32>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_alloc_returns_zeroed_data() {
        let mut seg = Segment::new(1024);
        let (offset, data) = seg.alloc(10).unwrap();
        assert_eq!(offset, 0);
        assert_eq!(data.len(), 10);
        assert!(data.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn segment_sequential_alloc() {
        let mut seg = Segment::new(1024);
        let (off1, _) = seg.alloc(100).unwrap();
        let (off2, _) = seg.alloc(200).unwrap();
        assert_eq!(off1, 0);
        assert_eq!(off2, 100);
        assert_eq!(seg.used(), 300);
    }

    #[test]
    fn segment_alloc_fails_when_full() {
        let mut seg = Segment::new(100);
        assert!(seg.alloc(100).is_some());
        assert!(seg.alloc(1).is_none());
    }

    #[test]
    fn segment_reset_allows_realloc() {
        let mut seg = Segment::new(100);
        seg.alloc(100).unwrap();
        seg.reset();
        assert_eq!(seg.used(), 0);
        assert!(seg.alloc(50).is_some());
    }

    #[test]
    fn segment_slice_reads_written_data() {
        let mut seg = Segment::new(1024);
        let (offset, data) = seg.alloc(5).unwrap();
        data[0] = 1.0;
        data[4] = 5.0;

        let read = seg.slice(offset, 5);
        assert_eq!(read[0], 1.0);
        assert_eq!(read[4], 5.0);
    }

    #[test]
    fn segment_list_alloc_within_first_segment() {
        let mut list = SegmentList::new(1024, 4);
        let (seg_idx, offset) = list.alloc(10).unwrap();
        assert_eq!(seg_idx, 0);
        assert_eq!(offset, 0);
    }

    #[test]
    fn segment_list_grows_on_overflow() {
        let mut list = SegmentList::new(100, 4);
        list.alloc(100).unwrap(); // fills first segment
        let (seg_idx, _) = list.alloc(50).unwrap(); // should go to second segment
        assert_eq!(seg_idx, 1);
        assert_eq!(list.segment_count(), 2);
    }

    #[test]
    fn segment_list_capacity_exceeded() {
        let mut list = SegmentList::new(100, 2);
        list.alloc(100).unwrap(); // fills segment 0
        list.alloc(100).unwrap(); // fills segment 1
        let result = list.alloc(1);
        assert!(matches!(result, Err(ArenaError::CapacityExceeded { .. })));
    }

    #[test]
    fn segment_list_reset() {
        let mut list = SegmentList::new(100, 4);
        list.alloc(80).unwrap();
        list.alloc(80).unwrap(); // triggers second segment
        assert_eq!(list.segment_count(), 2);
        list.reset();
        assert_eq!(list.total_used(), 0);
        // After reset, allocations start from beginning of first segment.
        let (seg_idx, offset) = list.alloc(10).unwrap();
        assert_eq!(seg_idx, 0);
        assert_eq!(offset, 0);
    }

    #[test]
    fn segment_list_slice_roundtrip() {
        let mut list = SegmentList::new(1024, 4);
        let (seg, off) = list.alloc(5).unwrap();
        {
            let s = list.slice_mut(seg, off, 5);
            s[0] = 42.0;
        }
        let read = list.slice(seg, off, 5);
        assert_eq!(read[0], 42.0);
    }

    #[test]
    fn oversized_alloc_returns_error_not_panic() {
        let mut list = SegmentList::new(100, 4);
        // Request more than segment_size — must return CapacityExceeded, not panic.
        let result = list.alloc(101);
        assert!(matches!(result, Err(ArenaError::CapacityExceeded { .. })));
    }

    #[test]
    fn exactly_segment_size_alloc_succeeds() {
        let mut list = SegmentList::new(100, 4);
        let result = list.alloc(100);
        assert!(result.is_ok());
    }
}
