use murk_space::{EdgeBehavior, Fcc12, Hex2D, Line1D, Ring1D, Space, Square4, Square8};

#[test]
fn square8_max_neighbour_degree_covers_absorb_and_wrap_clamp_paths() {
    let wrap = Square8::new(4, 4, EdgeBehavior::Wrap).unwrap();
    assert_eq!(wrap.max_neighbour_degree(), 8);

    let absorb_1x1 = Square8::new(1, 1, EdgeBehavior::Absorb).unwrap();
    assert_eq!(absorb_1x1.max_neighbour_degree(), 0);

    let absorb_1x2 = Square8::new(1, 2, EdgeBehavior::Absorb).unwrap();
    assert_eq!(absorb_1x2.max_neighbour_degree(), 1);

    let absorb_3x3 = Square8::new(3, 3, EdgeBehavior::Absorb).unwrap();
    assert_eq!(absorb_3x3.max_neighbour_degree(), 8);
}

#[test]
fn square8_canonical_rank_slice_handles_valid_and_invalid_coords() {
    let s = Square8::new(3, 4, EdgeBehavior::Clamp).unwrap();
    assert_eq!(s.canonical_rank_slice(&[2, 1]), Some(9));
    assert_eq!(s.canonical_rank_slice(&[2]), None);
    assert_eq!(s.canonical_rank_slice(&[4, 1]), None);
}

#[test]
fn square4_max_neighbour_degree_and_rank_slice_cover_new_branches() {
    let absorb_2x2 = Square4::new(2, 2, EdgeBehavior::Absorb).unwrap();
    assert_eq!(absorb_2x2.max_neighbour_degree(), 2);

    let clamp = Square4::new(3, 3, EdgeBehavior::Clamp).unwrap();
    assert_eq!(clamp.max_neighbour_degree(), 4);

    assert_eq!(clamp.canonical_rank_slice(&[1, 2]), Some(5));
    assert_eq!(clamp.canonical_rank_slice(&[1]), None);
    assert_eq!(clamp.canonical_rank_slice(&[1, 3]), None);
}

#[test]
fn line1d_max_neighbour_degree_and_rank_slice_cover_new_branches() {
    let clamp = Line1D::new(5, EdgeBehavior::Clamp).unwrap();
    assert_eq!(clamp.max_neighbour_degree(), 2);

    let absorb_1 = Line1D::new(1, EdgeBehavior::Absorb).unwrap();
    let absorb_2 = Line1D::new(2, EdgeBehavior::Absorb).unwrap();
    let absorb_5 = Line1D::new(5, EdgeBehavior::Absorb).unwrap();
    assert_eq!(absorb_1.max_neighbour_degree(), 0);
    assert_eq!(absorb_2.max_neighbour_degree(), 1);
    assert_eq!(absorb_5.max_neighbour_degree(), 2);

    assert_eq!(clamp.canonical_rank_slice(&[3]), Some(3));
    assert_eq!(clamp.canonical_rank_slice(&[3, 0]), None);
    assert_eq!(clamp.canonical_rank_slice(&[9]), None);
}

#[test]
fn ring1d_canonical_rank_slice_handles_valid_and_invalid_coords() {
    let ring = Ring1D::new(5).unwrap();
    assert_eq!(ring.canonical_rank_slice(&[4]), Some(4));
    assert_eq!(ring.canonical_rank_slice(&[4, 0]), None);
    assert_eq!(ring.canonical_rank_slice(&[5]), None);
}

#[test]
fn hex2d_max_neighbour_degree_covers_dimension_patterns() {
    let c11 = Hex2D::new(1, 1).unwrap();
    let c12 = Hex2D::new(2, 1).unwrap();
    let c31 = Hex2D::new(1, 3).unwrap();
    let c22 = Hex2D::new(2, 2).unwrap();
    let c24 = Hex2D::new(4, 2).unwrap();
    let c33 = Hex2D::new(3, 3).unwrap();

    assert_eq!(c11.max_neighbour_degree(), 0);
    assert_eq!(c12.max_neighbour_degree(), 1);
    assert_eq!(c31.max_neighbour_degree(), 2);
    assert_eq!(c22.max_neighbour_degree(), 3);
    assert_eq!(c24.max_neighbour_degree(), 4);
    assert_eq!(c33.max_neighbour_degree(), 6);
}

#[test]
fn hex2d_canonical_rank_slice_handles_valid_and_invalid_coords() {
    let hex = Hex2D::new(4, 3).unwrap();
    assert_eq!(hex.canonical_rank_slice(&[2, 1]), Some(5));
    assert_eq!(hex.canonical_rank_slice(&[2]), None);
    assert_eq!(hex.canonical_rank_slice(&[5, 1]), None);
}

#[test]
fn default_space_canonical_rank_slice_path_is_exercised_via_fcc12() {
    let fcc = Fcc12::new(2, 2, 2, EdgeBehavior::Absorb).unwrap();
    let space: &dyn Space = &fcc;

    assert_eq!(space.canonical_rank_slice(&[0, 0, 0]), Some(0));
    assert_eq!(space.canonical_rank_slice(&[0, 0]), None);
}
