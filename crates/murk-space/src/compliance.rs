//! Space trait compliance test helpers.
//!
//! These functions verify that a Space implementation satisfies the
//! invariants required by the trait contract. Reused across all backend
//! test modules (Line1D, Ring1D, Square4, Square8, Hex2D, ProductSpace).

use crate::region::RegionSpec;
use crate::space::Space;
use indexmap::IndexSet;

/// Assert that `distance(a, a) == 0.0` for all cells.
pub fn assert_distance_reflexive(space: &dyn Space) {
    for coord in space.canonical_ordering() {
        let d = space.distance(&coord, &coord);
        assert!(
            (d - 0.0).abs() < f64::EPSILON,
            "distance({coord:?}, {coord:?}) = {d}, expected 0.0"
        );
    }
}

/// Assert that `distance(a, b) == distance(b, a)` for all cell pairs.
pub fn assert_distance_symmetric(space: &dyn Space) {
    let cells = space.canonical_ordering();
    for a in &cells {
        for b in &cells {
            let dab = space.distance(a, b);
            let dba = space.distance(b, a);
            assert!(
                (dab - dba).abs() < f64::EPSILON,
                "distance({a:?}, {b:?}) = {dab} != distance({b:?}, {a:?}) = {dba}"
            );
        }
    }
}

/// Assert triangle inequality: `d(a, c) <= d(a, b) + d(b, c)` for all triples.
pub fn assert_distance_triangle_inequality(space: &dyn Space) {
    let cells = space.canonical_ordering();
    for a in &cells {
        for b in &cells {
            for c in &cells {
                let dac = space.distance(a, c);
                let dab = space.distance(a, b);
                let dbc = space.distance(b, c);
                assert!(
                    dac <= dab + dbc + f64::EPSILON,
                    "triangle inequality violated: d({a:?},{c:?})={dac} > d({a:?},{b:?})={dab} + d({b:?},{c:?})={dbc}"
                );
            }
        }
    }
}

/// Assert that `b in neighbours(a)` implies `a in neighbours(b)`.
pub fn assert_neighbours_symmetric(space: &dyn Space) {
    for coord in space.canonical_ordering() {
        for nb in space.neighbours(&coord) {
            let nb_neighbours = space.neighbours(&nb);
            assert!(
                nb_neighbours.contains(&coord),
                "neighbour symmetry violated: {nb:?} in N({coord:?}) but {coord:?} not in N({nb:?})"
            );
        }
    }
}

/// Assert that two calls to `canonical_ordering` return the same result.
pub fn assert_canonical_ordering_deterministic(space: &dyn Space) {
    let a = space.canonical_ordering();
    let b = space.canonical_ordering();
    assert_eq!(a, b, "canonical_ordering is non-deterministic");
}

/// Assert that `canonical_ordering` returns exactly `cell_count` unique coords.
pub fn assert_canonical_ordering_complete(space: &dyn Space) {
    let ordering = space.canonical_ordering();
    assert_eq!(
        ordering.len(),
        space.cell_count(),
        "canonical_ordering length ({}) != cell_count ({})",
        ordering.len(),
        space.cell_count()
    );
    let unique: IndexSet<_> = ordering.iter().collect();
    assert_eq!(
        unique.len(),
        space.cell_count(),
        "canonical_ordering has duplicates"
    );
}

/// Assert that `compile_region(All)` produces `valid_ratio == 1.0`.
pub fn assert_compile_region_all_valid_ratio(space: &dyn Space) {
    let plan = space
        .compile_region(&RegionSpec::All)
        .expect("compile_region(All) should succeed");
    let ratio = plan.valid_ratio();
    assert!(
        (ratio - 1.0).abs() < f64::EPSILON,
        "valid_ratio for All region = {ratio}, expected 1.0"
    );
}

/// Assert that `compile_region(All)` covers all cells.
pub fn assert_compile_region_all_covers_all(space: &dyn Space) {
    let plan = space
        .compile_region(&RegionSpec::All)
        .expect("compile_region(All) should succeed");
    assert_eq!(
        plan.cell_count,
        space.cell_count(),
        "compile_region(All).cell_count ({}) != space.cell_count ({})",
        plan.cell_count,
        space.cell_count()
    );
}

/// Run all 8 compliance checks on a space.
pub fn run_full_compliance(space: &dyn Space) {
    assert_distance_reflexive(space);
    assert_distance_symmetric(space);
    assert_distance_triangle_inequality(space);
    assert_neighbours_symmetric(space);
    assert_canonical_ordering_deterministic(space);
    assert_canonical_ordering_complete(space);
    assert_compile_region_all_valid_ratio(space);
    assert_compile_region_all_covers_all(space);
}
