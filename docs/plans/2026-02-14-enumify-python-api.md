# Enum-ify Python API Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace all magic integer parameters in the Python API with proper enum types so the API is self-documenting.

**Architecture:** Add new Python-side enums (`RegionType`, `TransformType`, `PoolKernel`, `DType`) to `config.rs`. Modify `ObsEntry.__init__`, `add_propagator`, `PropagatorDef.__init__`, and `Config.set_space` to accept enum types instead of (or alongside) raw ints. Since we're pre-1.0, we break the old int-based signatures cleanly — no backward compat shims.

**Tech Stack:** Rust (PyO3), Python (pytest)

---

### Task 1: Add new enums for ObsEntry parameters

**Files:**
- Modify: `crates/murk-python/src/config.rs` (append after existing enums, ~line 82)
- Modify: `crates/murk-python/src/lib.rs` (register new classes, ~line 23-41)
- Modify: `crates/murk-python/python/murk/__init__.py` (re-export new enums)

**Step 1: Write failing test — new enums exist and have correct values**

Add to `crates/murk-python/tests/test_config.py`:

```python
def test_region_type_enum_values():
    """RegionType enum has expected members."""
    from murk import RegionType
    assert RegionType.All.value == 0
    assert RegionType.AgentDisk.value == 5
    assert RegionType.AgentRect.value == 6


def test_transform_type_enum_values():
    """TransformType enum has expected members."""
    from murk import TransformType
    assert TransformType.Identity.value == 0
    assert TransformType.Normalize.value == 1


def test_pool_kernel_enum_values():
    """PoolKernel enum has expected members."""
    from murk import PoolKernel
    assert PoolKernel.NoPool.value == 0
    assert PoolKernel.Mean.value == 1
    assert PoolKernel.Max.value == 2
    assert PoolKernel.Min.value == 3
    assert PoolKernel.Sum.value == 4
```

**Step 2: Run test to verify it fails**

Run: `cd crates/murk-python && maturin develop --release 2>/dev/null && python -m pytest tests/test_config.py::test_region_type_enum_values -v`
Expected: FAIL (ImportError: cannot import 'RegionType')

**Step 3: Add the four new enums to config.rs**

Add after `EdgeBehavior` enum (after line 82 in `crates/murk-python/src/config.rs`):

```rust
/// Observation region type.
#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegionType {
    /// Full grid — observe every cell.
    All = 0,
    /// Circular patch around agent center.
    AgentDisk = 5,
    /// Rectangular patch around agent center.
    AgentRect = 6,
}

/// Observation transform applied at extraction time.
#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransformType {
    /// Raw field values, no transform.
    Identity = 0,
    /// Scale to [normalize_min, normalize_max] range.
    Normalize = 1,
}

/// Pooling kernel for observation downsampling.
#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PoolKernel {
    /// No pooling.
    NoPool = 0,
    /// Mean pooling.
    Mean = 1,
    /// Max pooling.
    Max = 2,
    /// Min pooling.
    Min = 3,
    /// Sum pooling.
    Sum = 4,
}

/// Observation data type.
#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DType {
    /// 32-bit float.
    F32 = 0,
}
```

Register in `crates/murk-python/src/lib.rs` (add to the `_murk` function, in the Enums section):

```rust
m.add_class::<config::RegionType>()?;
m.add_class::<config::TransformType>()?;
m.add_class::<config::PoolKernel>()?;
m.add_class::<config::DType>()?;
```

Re-export in `crates/murk-python/python/murk/__init__.py` — add to the import and `__all__`:

```python
# In the import block:
from murk._murk import (
    ...
    RegionType,
    TransformType,
    PoolKernel,
    DType,
    ...
)

# In __all__:
__all__ = [
    ...
    "RegionType",
    "TransformType",
    "PoolKernel",
    "DType",
    ...
]
```

**Step 4: Build and run tests**

Run: `cd crates/murk-python && maturin develop --release 2>/dev/null && python -m pytest tests/test_config.py -k "region_type or transform_type or pool_kernel" -v`
Expected: PASS (3 tests)

**Step 5: Commit**

```bash
git add crates/murk-python/src/config.rs crates/murk-python/src/lib.rs crates/murk-python/python/murk/__init__.py crates/murk-python/tests/test_config.py
git commit -m "feat(python): add RegionType, TransformType, PoolKernel, DType enums"
```

---

### Task 2: Update ObsEntry to accept enum types

**Files:**
- Modify: `crates/murk-python/src/obs.rs` (change `ObsEntry::new` signature, ~line 38-94)
- Modify: `crates/murk-python/tests/test_obs.py` (update existing tests + add new)
- Modify: `crates/murk-python/tests/conftest.py` (no change needed — uses positional int 0 which maps to All)

**Step 1: Write failing test — ObsEntry accepts enum types**

Add to `crates/murk-python/tests/test_obs.py`:

```python
def test_obsentry_accepts_enum_types():
    """ObsEntry accepts RegionType and TransformType enums."""
    from murk import ObsEntry, RegionType, TransformType, PoolKernel
    entry = ObsEntry(
        0,
        region_type=RegionType.All,
        transform_type=TransformType.Identity,
        pool_kernel=PoolKernel.NoPool,
    )
    # Should not raise


def test_obsentry_normalize_with_enum():
    """ObsEntry with TransformType.Normalize works end-to-end."""
    from murk import TransformType
    world, _ = make_const_world(value=5.0, n_cells=10)
    world.step()

    entries = [ObsEntry(0, transform_type=TransformType.Normalize,
                        normalize_min=0.0, normalize_max=10.0)]
    plan = ObsPlan(world, entries)

    obs = np.zeros(plan.output_len, dtype=np.float32)
    mask = np.zeros(plan.mask_len, dtype=np.uint8)
    plan.execute(world, obs, mask)

    np.testing.assert_allclose(obs, 0.5, rtol=1e-5)
    world.destroy()


def test_obsentry_rejects_raw_ints():
    """ObsEntry rejects raw int for region_type (must use enum)."""
    from murk import ObsEntry
    import pytest
    with pytest.raises(TypeError):
        ObsEntry(0, region_type=0)
```

**Step 2: Run test to verify it fails**

Run: `cd crates/murk-python && maturin develop --release 2>/dev/null && python -m pytest tests/test_obs.py::test_obsentry_accepts_enum_types -v`
Expected: FAIL (TypeError — PyO3 won't accept enum where i32 expected, or vice versa)

**Step 3: Update ObsEntry::new to accept enums**

In `crates/murk-python/src/obs.rs`, change the `new` method signature. Replace the `i32` params with the enum types:

```rust
use crate::config::{DType, PoolKernel, RegionType, TransformType};

#[new]
#[pyo3(signature = (
    field_id,
    region_type=RegionType::All,
    transform_type=TransformType::Identity,
    normalize_min=0.0,
    normalize_max=1.0,
    dtype=DType::F32,
    region_params=None,
    pool_kernel=PoolKernel::NoPool,
    pool_kernel_size=0,
    pool_stride=0,
))]
#[allow(clippy::too_many_arguments)]
fn new(
    field_id: u32,
    region_type: RegionType,
    transform_type: TransformType,
    normalize_min: f32,
    normalize_max: f32,
    dtype: DType,
    region_params: Option<Vec<i32>>,
    pool_kernel: PoolKernel,
    pool_kernel_size: i32,
    pool_stride: i32,
) -> PyResult<Self> {
    let mut params = [0i32; 8];
    let n_params = if let Some(ref rp) = region_params {
        if rp.len() > 8 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "region_params must have at most 8 elements",
            ));
        }
        for (i, &v) in rp.iter().enumerate() {
            params[i] = v;
        }
        rp.len() as i32
    } else {
        0
    };

    Ok(ObsEntry {
        inner: MurkObsEntry {
            field_id,
            region_type: region_type as i32,
            transform_type: transform_type as i32,
            normalize_min,
            normalize_max,
            dtype: dtype as i32,
            region_params: params,
            n_region_params: n_params,
            pool_kernel: pool_kernel as i32,
            pool_kernel_size,
            pool_stride,
        },
    })
}
```

**Step 4: Update existing test that passes raw int for transform_type**

In `crates/murk-python/tests/test_obs.py`, update `test_obsplan_normalize_transform`:

```python
def test_obsplan_normalize_transform():
    """Normalize transform scales values to [0, 1]."""
    from murk import TransformType
    world, _ = make_const_world(value=5.0, n_cells=10)
    world.step()

    entries = [ObsEntry(0, transform_type=TransformType.Normalize,
                        normalize_min=0.0, normalize_max=10.0)]
    plan = ObsPlan(world, entries)

    obs = np.zeros(plan.output_len, dtype=np.float32)
    mask = np.zeros(plan.mask_len, dtype=np.uint8)
    plan.execute(world, obs, mask)

    np.testing.assert_allclose(obs, 0.5, rtol=1e-5)
    world.destroy()
```

**Step 5: Build and run all obs tests**

Run: `cd crates/murk-python && maturin develop --release 2>/dev/null && python -m pytest tests/test_obs.py -v`
Expected: PASS (all tests)

**Step 6: Commit**

```bash
git add crates/murk-python/src/obs.rs crates/murk-python/tests/test_obs.py
git commit -m "feat(python): ObsEntry accepts enum types instead of raw ints"
```

---

### Task 3: Update PropagatorDef and add_propagator to use WriteMode enum

**Files:**
- Modify: `crates/murk-python/src/propagator.rs` (~line 43-44, 57-58, 151-152)
- Modify: `crates/murk-python/tests/test_propagator.py` (update write tuple syntax)
- Modify: `crates/murk-python/tests/conftest.py` (update write tuple syntax)

**Step 1: Write failing test — PropagatorDef accepts WriteMode enum**

Add to `crates/murk-python/tests/test_propagator.py`:

```python
def test_propagator_accepts_write_mode_enum():
    """PropagatorDef accepts WriteMode enum in write tuples."""
    from murk._murk import WriteMode

    def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
        writes[0][:] = 1.0

    cfg = Config()
    cfg.set_space(SpaceType.Line1D, [5.0, 0.0])
    cfg.add_field("x", mutability=FieldMutability.PerTick)
    cfg.set_dt(0.1)
    cfg.set_seed(0)

    prop = PropagatorDef("writer", step_fn, writes=[(0, WriteMode.Full)])
    prop.register(cfg)

    world = World(cfg)
    world.step()

    buf = np.zeros(5, dtype=np.float32)
    world.read_field(0, buf)
    np.testing.assert_array_equal(buf, 1.0)
    world.destroy()
```

**Step 2: Run test to verify it fails**

Run: `cd crates/murk-python && maturin develop --release 2>/dev/null && python -m pytest tests/test_propagator.py::test_propagator_accepts_write_mode_enum -v`
Expected: FAIL (TypeError — tuple element 1 expected int, got WriteMode)

**Step 3: Change writes type from `Vec<(u32, i32)>` to `Vec<(u32, WriteMode)>`**

In `crates/murk-python/src/propagator.rs`:

Add import at top:
```rust
use crate::command::WriteMode;
```

Change `PropagatorDef` struct (line ~43):
```rust
writes: Vec<(u32, WriteMode)>, // (field_id, write_mode)
```

Change `PropagatorDef::new` signature (line ~57):
```rust
#[new]
#[pyo3(signature = (name, step_fn, reads=vec![], reads_previous=vec![], writes=vec![]))]
fn new(
    name: String,
    step_fn: PyObject,
    reads: Vec<u32>,
    reads_previous: Vec<u32>,
    writes: Vec<(u32, WriteMode)>,
) -> Self {
```

In `register`, update the FFI write declarations (line ~91-98):
```rust
let ffi_writes: Vec<MurkWriteDecl> = self
    .writes
    .iter()
    .map(|(fid, mode)| MurkWriteDecl {
        field_id: *fid,
        mode: *mode as i32,
    })
    .collect();
```

Change `add_propagator` function signature (line ~151-152):
```rust
#[pyfunction]
#[pyo3(signature = (config, name, step_fn, reads=vec![], reads_previous=vec![], writes=vec![]))]
pub(crate) fn add_propagator(
    py: Python<'_>,
    config: &mut Config,
    name: String,
    step_fn: PyObject,
    reads: Vec<u32>,
    reads_previous: Vec<u32>,
    writes: Vec<(u32, WriteMode)>,
) -> PyResult<()> {
```

**Step 4: Update all existing tests and conftest to use WriteMode enum**

In `crates/murk-python/tests/conftest.py`, update both helpers:
- `writes=[(0, 0)]` → `writes=[(0, WriteMode.Full)]`
- Add `from murk._murk import WriteMode` to imports

In `crates/murk-python/tests/test_propagator.py`, update all:
- `writes=[(0, 0)]` → `writes=[(0, WriteMode.Full)]`
- `writes=[(0, 0), (1, 0)]` → `writes=[(0, WriteMode.Full), (1, WriteMode.Full)]`
- Add `WriteMode` to imports

In `crates/murk-python/tests/test_gymnasium.py`, update:
- `writes=[(0, 0)]` → `writes=[(0, WriteMode.Full)]`
- Add `WriteMode` to imports

**Step 5: Build and run all Python tests**

Run: `cd crates/murk-python && maturin develop --release 2>/dev/null && python -m pytest tests/ -v`
Expected: PASS (all tests)

**Step 6: Commit**

```bash
git add crates/murk-python/src/propagator.rs crates/murk-python/tests/
git commit -m "feat(python): PropagatorDef/add_propagator accept WriteMode enum"
```

---

### Task 4: Improve Config.set_space to accept EdgeBehavior in params

This is the trickiest change. The `set_space` params array is a flat `Vec<f64>` that gets passed through FFI — the edge behavior is encoded as a float (0.0, 1.0, 2.0) inside the array. The clean fix: add dedicated per-topology helper methods on Config that accept EdgeBehavior directly.

**Files:**
- Modify: `crates/murk-python/src/config.rs` (~line 111-130)
- Modify: `crates/murk-python/tests/test_config.py`

**Step 1: Write failing test — new typed space setter methods**

Add to `crates/murk-python/tests/test_config.py`:

```python
def test_config_set_space_line1d_typed():
    """set_space_line1d accepts EdgeBehavior enum."""
    cfg = Config()
    cfg.set_space_line1d(10, EdgeBehavior.Absorb)


def test_config_set_space_square4_typed():
    """set_space_square4 accepts EdgeBehavior enum."""
    cfg = Config()
    cfg.set_space_square4(5, 5, EdgeBehavior.Wrap)


def test_config_set_space_square8_typed():
    """set_space_square8 accepts EdgeBehavior enum."""
    cfg = Config()
    cfg.set_space_square8(8, 8, EdgeBehavior.Absorb)


def test_config_set_space_hex2d_typed():
    """set_space_hex2d accepts dimensions."""
    cfg = Config()
    cfg.set_space_hex2d(10, 10)


def test_config_set_space_ring1d_typed():
    """set_space_ring1d accepts length."""
    cfg = Config()
    cfg.set_space_ring1d(20)


def test_config_set_space_fcc12_typed():
    """set_space_fcc12 accepts dimensions and EdgeBehavior."""
    cfg = Config()
    cfg.set_space_fcc12(4, 4, 4, EdgeBehavior.Absorb)
```

**Step 2: Run test to verify it fails**

Run: `cd crates/murk-python && maturin develop --release 2>/dev/null && python -m pytest tests/test_config.py::test_config_set_space_line1d_typed -v`
Expected: FAIL (AttributeError: set_space_line1d)

**Step 3: Add typed space setter methods to Config**

In `crates/murk-python/src/config.rs`, add these methods inside the `#[pymethods] impl Config` block (after `set_space`):

```rust
/// Set space to Line1D.
///
/// Args:
///     length: Number of cells.
///     edge: Edge behavior (Absorb, Clamp, or Wrap).
fn set_space_line1d(&self, py: Python<'_>, length: u32, edge: EdgeBehavior) -> PyResult<()> {
    let params = vec![length as f64, edge as i32 as f64];
    self._set_space_raw(py, SpaceType::Line1D as i32, &params)
}

/// Set space to Ring1D (periodic 1D).
///
/// Args:
///     length: Number of cells.
fn set_space_ring1d(&self, py: Python<'_>, length: u32) -> PyResult<()> {
    let params = vec![length as f64];
    self._set_space_raw(py, SpaceType::Ring1D as i32, &params)
}

/// Set space to Square4 (2D grid, 4-connected).
///
/// Args:
///     width: Grid width.
///     height: Grid height.
///     edge: Edge behavior (Absorb, Clamp, or Wrap).
fn set_space_square4(
    &self,
    py: Python<'_>,
    width: u32,
    height: u32,
    edge: EdgeBehavior,
) -> PyResult<()> {
    let params = vec![width as f64, height as f64, edge as i32 as f64];
    self._set_space_raw(py, SpaceType::Square4 as i32, &params)
}

/// Set space to Square8 (2D grid, 8-connected).
///
/// Args:
///     width: Grid width.
///     height: Grid height.
///     edge: Edge behavior (Absorb, Clamp, or Wrap).
fn set_space_square8(
    &self,
    py: Python<'_>,
    width: u32,
    height: u32,
    edge: EdgeBehavior,
) -> PyResult<()> {
    let params = vec![width as f64, height as f64, edge as i32 as f64];
    self._set_space_raw(py, SpaceType::Square8 as i32, &params)
}

/// Set space to Hex2D (hexagonal lattice, 6-connected).
///
/// Args:
///     cols: Number of columns.
///     rows: Number of rows.
fn set_space_hex2d(&self, py: Python<'_>, cols: u32, rows: u32) -> PyResult<()> {
    let params = vec![cols as f64, rows as f64];
    self._set_space_raw(py, SpaceType::Hex2D as i32, &params)
}

/// Set space to Fcc12 (3D FCC lattice, 12-connected).
///
/// Args:
///     width: Grid width.
///     height: Grid height.
///     depth: Grid depth.
///     edge: Edge behavior (Absorb, Clamp, or Wrap).
fn set_space_fcc12(
    &self,
    py: Python<'_>,
    width: u32,
    height: u32,
    depth: u32,
    edge: EdgeBehavior,
) -> PyResult<()> {
    let params = vec![width as f64, height as f64, depth as f64, edge as i32 as f64];
    self._set_space_raw(py, SpaceType::Fcc12 as i32, &params)
}
```

Add a private helper (inside the non-pymethods `impl Config` block):

```rust
fn _set_space_raw(&self, py: Python<'_>, space_type: i32, params: &[f64]) -> PyResult<()> {
    let h = self.require_handle()?;
    let params_addr = params.as_ptr() as usize;
    let params_len = params.len();
    let status = py.allow_threads(|| {
        murk_config_set_space(h, space_type, params_addr as *const f64, params_len)
    });
    check_status(status)
}
```

Also refactor the existing `set_space` to use `_set_space_raw`:

```rust
/// Set the spatial topology (low-level).
///
/// Prefer the typed methods (set_space_square4, set_space_hex2d, etc.)
/// for a self-documenting API. This method is retained for ProductSpace
/// and advanced use cases.
fn set_space(&self, py: Python<'_>, space_type: SpaceType, params: Vec<f64>) -> PyResult<()> {
    self._set_space_raw(py, space_type as i32, &params)
}
```

**Step 4: Build and run tests**

Run: `cd crates/murk-python && maturin develop --release 2>/dev/null && python -m pytest tests/test_config.py -v`
Expected: PASS (all tests including new typed ones)

**Step 5: Commit**

```bash
git add crates/murk-python/src/config.rs crates/murk-python/tests/test_config.py
git commit -m "feat(python): add typed set_space_* methods accepting EdgeBehavior enum"
```

---

### Task 5: Update heat_seeker example to use new enum API

**Files:**
- Modify: `examples/heat_seeker/heat_seeker.py`
- Modify: `examples/heat_seeker/README.md`

**Step 1: Update heat_seeker.py imports and calls**

Replace magic numbers with enum names throughout. Key changes:

```python
# Old:
from murk import Command, Config, ObsEntry, SpaceType, FieldMutability, FieldType

config.set_space(SpaceType.Square4, [float(GRID_W), float(GRID_H), 0.0])
murk.add_propagator(config, ..., writes=[(HEAT_FIELD, 0)])
obs_entries = [ObsEntry(HEAT_FIELD), ObsEntry(AGENT_FIELD)]

# New:
from murk import (
    Command, Config, ObsEntry, FieldMutability, FieldType,
    EdgeBehavior, WriteMode, RegionType
)

config.set_space_square4(GRID_W, GRID_H, EdgeBehavior.Absorb)
murk.add_propagator(config, ..., writes=[(HEAT_FIELD, WriteMode.Full)])
obs_entries = [
    ObsEntry(HEAT_FIELD, region_type=RegionType.All),
    ObsEntry(AGENT_FIELD, region_type=RegionType.All),
]
```

**Step 2: Update README.md code snippets**

Replace code snippets showing `config.set_space(SpaceType.Square4, [16.0, 16.0, 0.0])` with the typed form. Update the edge behavior explanation to reference the enum.

**Step 3: Run the example (smoke test)**

Run: `cd /home/john/murk && python examples/heat_seeker/heat_seeker.py`
Expected: Runs without error (training output)

Note: Full PPO training takes ~40s. If just smoke-testing, Ctrl-C after a few seconds of output is sufficient.

**Step 4: Commit**

```bash
git add examples/heat_seeker/
git commit -m "docs: update heat_seeker example to use enum API"
```

---

### Task 6: Update remaining test files and run full test suite

**Files:**
- Modify: `crates/murk-python/tests/test_config.py` (update old set_space calls to typed form)
- Modify: `crates/murk-python/tests/test_gymnasium.py` (WriteMode + typed set_space)
- Modify: `crates/murk-python/tests/test_vec_env.py` (if it uses raw ints)
- Modify: `crates/murk-python/tests/test_ppo_smoke.py` (if it uses raw ints)
- Modify: `crates/murk-python/tests/test_gil_release.py` (if it uses raw ints)

**Step 1: Audit all test files for remaining raw int usage**

Search for patterns: `set_space(SpaceType.`, `writes=[(`, `region_type=0`, `transform_type=1`.

For each occurrence:
- `set_space(SpaceType.Square4, [W, H, 0.0])` → `set_space_square4(W, H, EdgeBehavior.Absorb)` (etc)
- `writes=[(N, 0)]` → `writes=[(N, WriteMode.Full)]`
- `region_type=0` → `region_type=RegionType.All` (already handled if ObsEntry defaults work)

**Step 2: Build and run the full test suite**

Run: `cd crates/murk-python && maturin develop --release 2>/dev/null && python -m pytest tests/ -v`
Expected: ALL PASS

**Step 3: Run Rust tests to ensure no regressions**

Run: `cargo test --workspace`
Expected: ALL PASS

**Step 4: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: No warnings

**Step 5: Commit**

```bash
git add crates/murk-python/tests/
git commit -m "test: migrate all Python tests to enum-based API"
```

---

### Task 7: Deprecation note on raw set_space

**Files:**
- Modify: `crates/murk-python/src/config.rs` (docstring update only)

**Step 1: Add deprecation note to set_space docstring**

The old `set_space(space_type, params)` method still works for `ProductSpace` (which needs the packed params format). Mark it as low-level in the docstring — already done in Task 4.

No code change needed beyond what Task 4 already did. Verify the docstring reads:

```rust
/// Set the spatial topology (low-level).
///
/// Prefer the typed methods (set_space_square4, set_space_hex2d, etc.)
/// for a self-documenting API. This method is retained for ProductSpace
/// and advanced use cases.
```

**Step 2: Final full test run**

Run: `cd crates/murk-python && maturin develop --release 2>/dev/null && python -m pytest tests/ -v && cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: ALL PASS, no warnings

**Step 3: Commit (if any docstring changes)**

```bash
git add crates/murk-python/src/config.rs
git commit -m "docs: mark set_space as low-level, prefer typed methods"
```
