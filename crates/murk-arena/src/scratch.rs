//! Per-tick scratch space for temporary propagator allocations.
//!
//! [`ScratchRegion`] is a bump allocator over a `Vec<f32>`. It is reset
//! between propagator invocations by the tick engine, so each propagator
//! sees a fresh, empty scratch region. The backing allocation is reused
//! across ticks to avoid repeated heap allocation.

/// Bump-allocated scratch space for temporary per-propagator data.
///
/// Propagators can request temporary f32 slices from scratch space for
/// intermediate calculations (e.g. neighbourhood sums, gradient buffers).
/// The scratch region is reset between propagator executions within a
/// single tick — allocations do not persist across propagators or ticks.
///
/// # Example (conceptual)
///
/// ```ignore
/// let slice = scratch.alloc(100)?;
/// // use slice for intermediate computation
/// // scratch.reset() called by engine before next propagator
/// ```
pub struct ScratchRegion {
    /// Backing storage. Grows on demand, never shrinks during runtime.
    data: Vec<f32>,
    /// Current bump pointer (number of f32 elements allocated so far).
    cursor: usize,
}

impl ScratchRegion {
    /// Create a new scratch region with the given initial capacity (in f32 elements).
    pub fn new(initial_capacity: usize) -> Self {
        Self {
            data: vec![0.0; initial_capacity],
            cursor: 0,
        }
    }

    /// Allocate `len` f32 elements from scratch space.
    ///
    /// Returns a zero-initialised mutable slice. Returns `None` if the
    /// scratch region cannot grow to accommodate the request (this should
    /// not happen in practice since `Vec` grows on demand; `None` is
    /// returned only if the system is out of memory).
    pub fn alloc(&mut self, len: usize) -> Option<&mut [f32]> {
        let new_cursor = self.cursor.checked_add(len)?;
        if new_cursor > self.data.len() {
            // Grow to at least double or the required size, whichever is larger.
            let new_cap = self.data.len().max(1024).max(new_cursor).checked_mul(2).unwrap_or(new_cursor);
            self.data.resize(new_cap, 0.0);
        }
        let start = self.cursor;
        self.cursor = new_cursor;
        // Zero-init the newly allocated region (may have stale data from previous use).
        let slice = &mut self.data[start..new_cursor];
        slice.fill(0.0);
        Some(slice)
    }

    /// Reset the scratch region for the next propagator.
    ///
    /// This does NOT deallocate or zero the backing storage — it simply
    /// resets the bump pointer. The next `alloc` call will overwrite
    /// stale data with zeroes before returning.
    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    /// Number of f32 elements currently allocated.
    pub fn used(&self) -> usize {
        self.cursor
    }

    /// Total capacity of the backing storage in f32 elements.
    pub fn capacity(&self) -> usize {
        self.data.len()
    }

    /// Memory usage of the backing storage in bytes.
    pub fn memory_bytes(&self) -> usize {
        self.data.len() * std::mem::size_of::<f32>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_returns_zeroed_slice() {
        let mut scratch = ScratchRegion::new(1024);
        let s = scratch.alloc(10).unwrap();
        assert_eq!(s.len(), 10);
        assert!(s.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sequential_allocs_dont_overlap() {
        let mut scratch = ScratchRegion::new(1024);
        let a = scratch.alloc(5).unwrap();
        a[0] = 1.0;
        a[4] = 5.0;
        let a_ptr = a.as_ptr();

        let b = scratch.alloc(3).unwrap();
        b[0] = 10.0;
        let b_ptr = b.as_ptr();

        // Pointers should not overlap (b starts after a).
        assert_ne!(a_ptr, b_ptr);
        assert_eq!(scratch.used(), 8);
    }

    #[test]
    fn reset_allows_reuse() {
        let mut scratch = ScratchRegion::new(1024);
        scratch.alloc(100).unwrap();
        assert_eq!(scratch.used(), 100);

        scratch.reset();
        assert_eq!(scratch.used(), 0);

        // Re-alloc after reset should return zeroed data.
        let s = scratch.alloc(50).unwrap();
        assert_eq!(s.len(), 50);
        assert!(s.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn grows_beyond_initial_capacity() {
        let mut scratch = ScratchRegion::new(10);
        let s = scratch.alloc(100).unwrap();
        assert_eq!(s.len(), 100);
        assert!(scratch.capacity() >= 100);
    }

    #[test]
    fn zero_alloc_is_valid() {
        let mut scratch = ScratchRegion::new(1024);
        let s = scratch.alloc(0).unwrap();
        assert!(s.is_empty());
        assert_eq!(scratch.used(), 0);
    }

    #[test]
    fn memory_bytes_tracks_capacity() {
        let scratch = ScratchRegion::new(1024);
        assert_eq!(scratch.memory_bytes(), 1024 * 4);
    }

    #[test]
    fn growth_overflow_falls_back_to_exact_fit() {
        // When the doubling multiplication would overflow usize,
        // the allocator should fall back to exact-fit (new_cursor).
        // We can't actually allocate usize::MAX/2 f32s, but we can
        // verify the capacity calculation doesn't panic.
        let mut scratch = ScratchRegion::new(0);
        // First alloc triggers growth from 0 → at least 1024 * 2.
        let s = scratch.alloc(10).unwrap();
        assert_eq!(s.len(), 10);
        // The capacity should be at least 2048 (1024 min * 2).
        assert!(scratch.capacity() >= 2048);
    }
}
