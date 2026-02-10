//! Debug-mode write coverage tracking for `WriteMode::Full` fields.
//!
//! [`FullWriteGuard`] wraps a mutable field buffer and, in debug builds,
//! tracks which cells have been written. On drop it logs a diagnostic if
//! coverage is incomplete. Release builds pay zero overhead.

use murk_core::FieldId;

/// Guard that tracks write coverage for [`WriteMode::Full`](crate::WriteMode) fields.
///
/// In debug builds, maintains a boolean vector tracking which cells have
/// been written. On drop, logs a warning if coverage is incomplete.
///
/// In release builds, this is a transparent wrapper with zero overhead.
pub struct FullWriteGuard<'a> {
    data: &'a mut [f32],
    #[cfg(debug_assertions)]
    written: Vec<bool>,
    #[cfg(debug_assertions)]
    propagator_name: String,
    #[cfg(debug_assertions)]
    field_id: FieldId,
}

impl<'a> FullWriteGuard<'a> {
    /// Create a new guard wrapping a mutable field buffer.
    ///
    /// `propagator_name` and `field_id` are used for diagnostic messages
    /// in debug builds.
    pub fn new(
        data: &'a mut [f32],
        #[cfg_attr(not(debug_assertions), allow(unused_variables))] propagator_name: &str,
        #[cfg_attr(not(debug_assertions), allow(unused_variables))] field_id: FieldId,
    ) -> Self {
        Self {
            #[cfg(debug_assertions)]
            written: vec![false; data.len()],
            #[cfg(debug_assertions)]
            propagator_name: propagator_name.to_string(),
            #[cfg(debug_assertions)]
            field_id,
            data,
        }
    }

    /// Write a single value at the given index.
    pub fn write_at(&mut self, index: usize, value: f32) {
        self.data[index] = value;
        #[cfg(debug_assertions)]
        {
            self.written[index] = true;
        }
    }

    /// Get the underlying slice for bulk writes.
    ///
    /// Marks ALL cells as written — assumes the caller fills the entire slice.
    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        #[cfg(debug_assertions)]
        {
            self.written.fill(true);
        }
        self.data
    }

    /// Number of cells in the buffer.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns `true` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Fraction of cells written (always 1.0 in release builds).
    pub fn coverage(&self) -> f64 {
        #[cfg(debug_assertions)]
        {
            if self.data.is_empty() {
                return 1.0;
            }
            let count = self.written.iter().filter(|&&b| b).count();
            count as f64 / self.data.len() as f64
        }
        #[cfg(not(debug_assertions))]
        {
            1.0
        }
    }

    /// Explicitly mark the guard as complete, suppressing the drop diagnostic.
    pub fn mark_complete(&mut self) {
        #[cfg(debug_assertions)]
        {
            self.written.fill(true);
        }
    }
}

#[cfg(debug_assertions)]
impl Drop for FullWriteGuard<'_> {
    fn drop(&mut self) {
        if self.data.is_empty() {
            return;
        }
        let total = self.written.len();
        let count = self.written.iter().filter(|&&b| b).count();
        if count < total {
            eprintln!(
                "murk: FullWriteGuard incomplete — propagator '{}', field {:?}: {}/{} written ({:.1}%)",
                self.propagator_name,
                self.field_id,
                count,
                total,
                (count as f64 / total as f64) * 100.0,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_coverage_no_warning() {
        let mut buf = vec![0.0; 4];
        let mut guard = FullWriteGuard::new(&mut buf, "test", FieldId(0));
        for i in 0..4 {
            guard.write_at(i, i as f32);
        }
        assert_eq!(guard.coverage(), 1.0);
    }

    #[test]
    fn partial_coverage_detected() {
        let mut buf = vec![0.0; 4];
        let mut guard = FullWriteGuard::new(&mut buf, "test", FieldId(0));
        guard.write_at(0, 1.0);
        guard.write_at(2, 3.0);
        assert_eq!(guard.coverage(), 0.5);
    }

    #[test]
    fn as_mut_slice_marks_complete() {
        let mut buf = vec![0.0; 4];
        let mut guard = FullWriteGuard::new(&mut buf, "test", FieldId(0));
        let slice = guard.as_mut_slice();
        slice.fill(1.0);
        assert_eq!(guard.coverage(), 1.0);
    }

    #[test]
    fn mark_complete_suppresses_warning() {
        let mut buf = vec![0.0; 4];
        let mut guard = FullWriteGuard::new(&mut buf, "test", FieldId(0));
        guard.write_at(0, 1.0);
        guard.mark_complete();
        assert_eq!(guard.coverage(), 1.0);
    }

    #[test]
    fn empty_buffer_full_coverage() {
        let mut buf: Vec<f32> = vec![];
        let guard = FullWriteGuard::new(&mut buf, "test", FieldId(0));
        assert!(guard.is_empty());
        assert_eq!(guard.coverage(), 1.0);
    }

    #[test]
    fn writes_visible_in_buffer() {
        let mut buf = vec![0.0; 3];
        {
            let mut guard = FullWriteGuard::new(&mut buf, "test", FieldId(0));
            guard.write_at(0, 10.0);
            guard.write_at(1, 20.0);
            guard.write_at(2, 30.0);
        }
        assert_eq!(buf, vec![10.0, 20.0, 30.0]);
    }
}
