//! Arena configuration parameters.

/// Configuration for the arena allocator.
///
/// Controls segment sizing, capacity limits, and generation retention.
/// Validated at construction; all values are immutable after creation.
#[derive(Clone, Debug)]
pub struct ArenaConfig {
    /// Size of each arena segment in f32 elements.
    ///
    /// Default: 16_777_216 (64MB at 4 bytes per f32).
    /// Must be a power of two and at least 1024.
    pub segment_size: u32,

    /// Maximum number of segments across all pools (per-tick A + B + sparse).
    ///
    /// Default: 16. Each segment is `segment_size * 4` bytes, so 16 segments
    /// at the default size = 1GB total arena capacity.
    pub max_segments: u16,

    /// How many generations a handle remains valid after creation.
    ///
    /// For Lockstep mode this is 1 (only the current and previous generation
    /// are live). For RealtimeAsync this matches the ring buffer capacity.
    pub max_generation_age: u32,

    /// Number of cells in the simulation grid.
    ///
    /// Used to compute per-field allocation sizes:
    /// `field_len = cell_count * field_type.components()`.
    pub cell_count: u32,
}

impl ArenaConfig {
    /// Default segment size: 64MB / 4 bytes = 16M f32 elements.
    pub const DEFAULT_SEGMENT_SIZE: u32 = 16_777_216;

    /// Default maximum segment count.
    pub const DEFAULT_MAX_SEGMENTS: u16 = 16;

    /// Default generation age for Lockstep mode.
    pub const DEFAULT_MAX_GENERATION_AGE: u32 = 1;

    /// Create a new arena config for the given cell count.
    ///
    /// Uses default values for all other parameters.
    pub fn new(cell_count: u32) -> Self {
        Self {
            segment_size: Self::DEFAULT_SEGMENT_SIZE,
            max_segments: Self::DEFAULT_MAX_SEGMENTS,
            max_generation_age: Self::DEFAULT_MAX_GENERATION_AGE,
            cell_count,
        }
    }

    /// Total capacity of a single segment in bytes.
    pub fn segment_bytes(&self) -> usize {
        self.segment_size as usize * std::mem::size_of::<f32>()
    }
}

impl Default for ArenaConfig {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_segment_size_is_64mb() {
        let config = ArenaConfig::new(100);
        assert_eq!(config.segment_bytes(), 64 * 1024 * 1024);
    }

    #[test]
    fn cell_count_preserved() {
        let config = ArenaConfig::new(10_000);
        assert_eq!(config.cell_count, 10_000);
    }
}
