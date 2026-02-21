//! Pre-allocated scratch memory for propagators.
//!
//! Each propagator declares [`scratch_bytes()`](crate::Propagator::scratch_bytes)
//! at registration. The engine pre-allocates the maximum across all propagators
//! and resets the bump pointer between each `step()` call.

/// Bump-allocated scratch region reset between propagators.
///
/// Prevents heap allocation in the inner loop. All scratch data is `f32`
/// (matching field storage), allocated as contiguous slices.
pub struct ScratchRegion {
    buf: Vec<f32>,
    offset: usize,
}

impl ScratchRegion {
    /// Create a new scratch region with the given capacity **in f32 slots**
    /// (not bytes).
    ///
    /// If you have a byte count (e.g., from `Propagator::scratch_bytes()`),
    /// use [`with_byte_capacity()`](Self::with_byte_capacity) instead.
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0.0; capacity],
            offset: 0,
        }
    }

    /// Create from a **byte** capacity (rounded up to whole f32 slots).
    ///
    /// This is the correct constructor when using the value from
    /// [`Propagator::scratch_bytes()`](crate::Propagator::scratch_bytes).
    pub fn with_byte_capacity(bytes: usize) -> Self {
        let slot_size = std::mem::size_of::<f32>();
        // Overflow-safe ceiling division (avoids bytes + slot_size - 1 wrapping).
        Self::new(bytes / slot_size + usize::from(!bytes.is_multiple_of(slot_size)))
    }

    /// Allocate `count` contiguous f32 slots, zero-initialized.
    ///
    /// Returns `None` if insufficient capacity remains.
    pub fn alloc(&mut self, count: usize) -> Option<&mut [f32]> {
        let new_offset = self.offset.checked_add(count)?;
        if new_offset > self.buf.len() {
            return None;
        }
        let start = self.offset;
        self.offset = new_offset;
        self.buf[start..new_offset].fill(0.0);
        Some(&mut self.buf[start..new_offset])
    }

    /// Reset the bump pointer. Called between propagators.
    pub fn reset(&mut self) {
        self.offset = 0;
    }

    /// Total capacity in f32 slots.
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Slots used since last reset.
    pub fn used(&self) -> usize {
        self.offset
    }

    /// Remaining available slots.
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_and_reset() {
        let mut s = ScratchRegion::new(10);
        assert_eq!(s.capacity(), 10);
        assert_eq!(s.remaining(), 10);

        let a = s.alloc(4).unwrap();
        assert_eq!(a.len(), 4);
        assert!(a.iter().all(|&v| v == 0.0));
        assert_eq!(s.used(), 4);
        assert_eq!(s.remaining(), 6);

        let b = s.alloc(6).unwrap();
        assert_eq!(b.len(), 6);
        assert_eq!(s.remaining(), 0);

        assert!(s.alloc(1).is_none());

        s.reset();
        assert_eq!(s.used(), 0);
        assert_eq!(s.remaining(), 10);
    }

    #[test]
    fn from_byte_capacity() {
        let s = ScratchRegion::with_byte_capacity(16);
        assert_eq!(s.capacity(), 4); // 16 bytes / 4 bytes per f32
    }

    #[test]
    fn from_byte_capacity_rounds_up() {
        // 5 bytes should yield 2 f32 slots (8 bytes), not 1 (4 bytes).
        let s = ScratchRegion::with_byte_capacity(5);
        assert_eq!(s.capacity(), 2);
        // 1 byte should still get 1 slot.
        let s = ScratchRegion::with_byte_capacity(1);
        assert_eq!(s.capacity(), 1);
        // 0 bytes is fine — 0 slots.
        let s = ScratchRegion::with_byte_capacity(0);
        assert_eq!(s.capacity(), 0);
        // Exact multiple is unchanged.
        let s = ScratchRegion::with_byte_capacity(8);
        assert_eq!(s.capacity(), 2);
    }

    #[test]
    fn from_byte_capacity_no_overflow_at_usize_max() {
        // The old (bytes + slot_size - 1) / slot_size formula would overflow.
        // Verify the safe formula produces the correct ceiling division.
        let slot_size = std::mem::size_of::<f32>(); // 4
        let expected = usize::MAX / slot_size + 1; // ceil(usize::MAX / 4)
                                                   // We can't actually allocate this, but verify the arithmetic is correct.
                                                   // Use the same formula as with_byte_capacity directly:
        let slots = usize::MAX / slot_size + usize::from(usize::MAX % slot_size != 0);
        assert_eq!(slots, expected);
    }

    #[test]
    fn zero_capacity() {
        let mut s = ScratchRegion::new(0);
        assert!(s.alloc(1).is_none());
        assert_eq!(s.capacity(), 0);
    }

    #[test]
    fn alloc_zero_slots() {
        let mut s = ScratchRegion::new(4);
        let a = s.alloc(0).unwrap();
        assert!(a.is_empty());
        assert_eq!(s.used(), 0);
    }

    #[test]
    fn writes_are_visible() {
        let mut s = ScratchRegion::new(4);
        let a = s.alloc(4).unwrap();
        a[0] = 1.0;
        a[3] = 4.0;

        // After reset, re-alloc returns zeroed data
        s.reset();
        let b = s.alloc(4).unwrap();
        assert_eq!(b, &[0.0, 0.0, 0.0, 0.0]);
    }
}
