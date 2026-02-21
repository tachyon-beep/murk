//! Generic slot+generation handle table for FFI lifetime management.
//!
//! Prevents use-after-free across the C boundary: destroyed handles have
//! stale generation counters and safely return `None` instead of causing UB.
//! Double-destroy is a safe no-op (returns `None`).

/// Handle encoding: upper 32 bits = slot index, lower 32 bits = generation.
fn encode(slot: u32, generation: u32) -> u64 {
    ((slot as u64) << 32) | (generation as u64)
}

fn decode(handle: u64) -> (u32, u32) {
    let slot = (handle >> 32) as u32;
    let generation = handle as u32;
    (slot, generation)
}

struct Slot<T> {
    generation: u32,
    data: Option<T>,
}

/// A slot+generation handle table mapping `u64` handles to owned values.
///
/// Reuses slots via a free list. Generation counters increment on removal,
/// making stale handles detectable without UB.
pub(crate) struct HandleTable<T> {
    slots: Vec<Slot<T>>,
    free_list: Vec<u32>,
}

impl<T> HandleTable<T> {
    /// Create an empty handle table.
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_list: Vec::new(),
        }
    }

    /// Insert a value and return its handle.
    pub fn insert(&mut self, value: T) -> u64 {
        if let Some(slot_idx) = self.free_list.pop() {
            let slot = &mut self.slots[slot_idx as usize];
            slot.data = Some(value);
            encode(slot_idx, slot.generation)
        } else {
            let slot_idx = self.slots.len() as u32;
            self.slots.push(Slot {
                generation: 0,
                data: Some(value),
            });
            encode(slot_idx, 0)
        }
    }

    /// Get an immutable reference to the value behind a handle.
    ///
    /// Returns `None` if the handle is stale (wrong generation) or was never valid.
    pub fn get(&self, handle: u64) -> Option<&T> {
        let (slot_idx, generation) = decode(handle);
        let slot = self.slots.get(slot_idx as usize)?;
        if slot.generation != generation {
            return None;
        }
        slot.data.as_ref()
    }

    /// Get a mutable reference to the value behind a handle.
    ///
    /// Returns `None` if the handle is stale or invalid.
    pub fn get_mut(&mut self, handle: u64) -> Option<&mut T> {
        let (slot_idx, generation) = decode(handle);
        let slot = self.slots.get_mut(slot_idx as usize)?;
        if slot.generation != generation {
            return None;
        }
        slot.data.as_mut()
    }

    /// Remove the value behind a handle, returning it.
    ///
    /// Increments the generation counter and adds the slot to the free list.
    /// If the generation has reached `u32::MAX`, the slot is permanently retired
    /// (not returned to the free list) to prevent ABA handle resurrection after
    /// wraparound.
    /// Returns `None` if the handle is stale (double-remove is safe).
    pub fn remove(&mut self, handle: u64) -> Option<T> {
        let (slot_idx, generation) = decode(handle);
        let slot = self.slots.get_mut(slot_idx as usize)?;
        if slot.generation != generation {
            return None;
        }
        let value = slot.data.take()?;
        slot.generation = slot.generation.wrapping_add(1);
        // Only recycle the slot if the generation hasn't wrapped back to 0.
        // A wrapped generation would collide with stale handles from epoch 0,
        // enabling ABA resurrection. Retiring the slot sacrifices ~32 bytes
        // to guarantee handle safety.
        if slot.generation != 0 {
            self.free_list.push(slot_idx);
        }
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_get_round_trip() {
        let mut table = HandleTable::new();
        let h = table.insert(42i32);
        assert_eq!(table.get(h), Some(&42));
    }

    #[test]
    fn get_mut_modifies_value() {
        let mut table = HandleTable::new();
        let h = table.insert(10i32);
        *table.get_mut(h).unwrap() = 20;
        assert_eq!(table.get(h), Some(&20));
    }

    #[test]
    fn remove_returns_value() {
        let mut table = HandleTable::new();
        let h = table.insert(99i32);
        assert_eq!(table.remove(h), Some(99));
        assert_eq!(table.get(h), None);
    }

    #[test]
    fn stale_generation_returns_none() {
        let mut table = HandleTable::new();
        let h = table.insert(1i32);
        table.remove(h);
        // Stale handle
        assert_eq!(table.get(h), None);
        assert_eq!(table.get_mut(h), None);
    }

    #[test]
    fn double_remove_returns_none() {
        let mut table = HandleTable::new();
        let h = table.insert(1i32);
        assert_eq!(table.remove(h), Some(1));
        assert_eq!(table.remove(h), None); // no panic
    }

    #[test]
    fn free_list_reuses_slots() {
        let mut table = HandleTable::new();
        let h1 = table.insert(1i32);
        table.remove(h1);
        let h2 = table.insert(2i32);
        // Slot reused, but different generation.
        let (slot1, gen1) = decode(h1);
        let (slot2, gen2) = decode(h2);
        assert_eq!(slot1, slot2);
        assert_ne!(gen1, gen2);
        assert_eq!(table.get(h2), Some(&2));
        // Old handle is stale.
        assert_eq!(table.get(h1), None);
    }

    #[test]
    fn generation_increments_on_remove() {
        let mut table = HandleTable::new();
        let h1 = table.insert(1i32);
        let (_, gen1) = decode(h1);
        table.remove(h1);
        let h2 = table.insert(2i32);
        let (_, gen2) = decode(h2);
        assert_eq!(gen2, gen1 + 1);
    }

    #[test]
    fn invalid_handle_returns_none() {
        let table: HandleTable<i32> = HandleTable::new();
        // Handle pointing to slot that never existed.
        assert_eq!(table.get(encode(999, 0)), None);
    }

    #[test]
    fn generation_exhaustion_retires_slot() {
        let mut table = HandleTable::new();
        let h = table.insert(1i32);
        table.remove(h);

        // Fast-forward: set slot 0's generation to u32::MAX - 1 directly,
        // then do one insert+remove cycle to reach u32::MAX, then one more
        // remove to trigger the wrap guard.
        table.slots[0].generation = u32::MAX - 1;
        let h2 = table.insert(2i32);
        let (_, gen2) = decode(h2);
        assert_eq!(gen2, u32::MAX - 1);

        // Remove bumps generation to u32::MAX, slot still recyclable.
        table.remove(h2);
        assert_eq!(table.slots[0].generation, u32::MAX);
        assert!(table.free_list.contains(&0));

        // Insert at generation u32::MAX.
        let h3 = table.insert(3i32);
        let (_, gen3) = decode(h3);
        assert_eq!(gen3, u32::MAX);

        // Remove wraps generation to 0 — slot must NOT be recycled.
        table.remove(h3);
        assert_eq!(table.slots[0].generation, 0);
        assert!(
            !table.free_list.contains(&0),
            "slot with wrapped generation must be retired, not recycled"
        );

        // Stale handle from the first epoch must not resolve to new data.
        let stale = encode(0, 0);
        assert_eq!(
            table.get(stale),
            None,
            "stale handle with generation 0 must not match retired slot"
        );

        // New insert must allocate a fresh slot instead of reusing slot 0.
        let h4 = table.insert(4i32);
        let (slot4, _) = decode(h4);
        assert_ne!(slot4, 0, "retired slot must not be reused");
    }
}
