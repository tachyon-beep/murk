//! C-compatible enums for space types, field properties, and write modes.

/// Spatial topology type for `murk_config_set_space`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MurkSpaceType {
    /// 1D line with configurable edge behavior.
    Line1D = 0,
    /// 1D ring (always-wrap periodic boundary).
    Ring1D = 1,
    /// 2D grid, 4-connected (N/S/E/W).
    Square4 = 2,
    /// 2D grid, 8-connected (+ diagonals).
    Square8 = 3,
    /// 2D hexagonal lattice, 6-connected (pointy-top).
    Hex2D = 4,
    /// Cartesian product of arbitrary spaces.
    ProductSpace = 5,
}

/// Field allocation strategy across ticks.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MurkFieldMutability {
    /// Generation 0 forever.
    Static = 0,
    /// New allocation each tick if modified.
    PerTick = 1,
    /// New allocation only when modified.
    Sparse = 2,
}

/// Field data type classification.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MurkFieldType {
    /// Single f32 per cell.
    Scalar = 0,
    /// Fixed-size f32 vector per cell.
    Vector = 1,
    /// Categorical (discrete) value per cell.
    Categorical = 2,
}

/// Write initialization strategy.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MurkWriteMode {
    /// Fresh buffer — propagator must fill every cell.
    Full = 0,
    /// Seeded from previous generation; propagator updates selectively.
    Incremental = 1,
}

/// Boundary behavior when field values exceed bounds.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MurkBoundaryBehavior {
    /// Clamp to nearest bound.
    Clamp = 0,
    /// Reflect off the bound.
    Reflect = 1,
    /// Absorb at the boundary.
    Absorb = 2,
    /// Wrap around to opposite bound.
    Wrap = 3,
}

/// Edge behavior for 1D/2D lattice spaces.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MurkEdgeBehavior {
    /// Absorb: cells at edge have no neighbor beyond.
    Absorb = 0,
    /// Clamp: beyond-edge neighbors map to edge cell.
    Clamp = 1,
    /// Wrap: periodic boundary.
    Wrap = 2,
}
