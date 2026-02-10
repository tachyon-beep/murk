//! Reusable propagator test fixtures.
//!
//! Three standard propagators for pipeline validation and engine testing:
//!
//! - [`IdentityPropagator`] — copies input field to output field (Full mode).
//! - [`ConstPropagator`] — writes a constant value (Full mode, no reads).
//! - [`FailingPropagator`] — fails deterministically after N calls.

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Reads one field and copies it to another (Full write mode).
///
/// Useful for testing pipeline routing: if the output matches the input,
/// the overlay is working correctly.
pub struct IdentityPropagator {
    pub name: String,
    pub input: FieldId,
    pub output: FieldId,
}

impl IdentityPropagator {
    pub fn new(name: impl Into<String>, input: FieldId, output: FieldId) -> Self {
        Self {
            name: name.into(),
            input,
            output,
        }
    }
}

impl Propagator for IdentityPropagator {
    fn name(&self) -> &str {
        &self.name
    }

    fn reads(&self) -> FieldSet {
        [self.input].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.output, WriteMode::Full)]
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let input = ctx.reads().read(self.input).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: format!("field {:?} not readable", self.input),
            }
        })?;
        let input_copy: Vec<f32> = input.to_vec();
        let output = ctx.writes().write(self.output).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: format!("field {:?} not writable", self.output),
            }
        })?;
        if output.len() != input_copy.len() {
            return Err(PropagatorError::ExecutionFailed {
                reason: format!(
                    "size mismatch: input field {:?} has {} elements, \
                     output field {:?} has {}",
                    self.input,
                    input_copy.len(),
                    self.output,
                    output.len(),
                ),
            });
        }
        output.copy_from_slice(&input_copy);
        Ok(())
    }
}

/// Writes a constant value to all cells (Full write mode, no reads).
///
/// Useful for testing that write buffers are correctly allocated and
/// that downstream propagators see the written values.
pub struct ConstPropagator {
    pub name: String,
    pub output: FieldId,
    pub value: f32,
}

impl ConstPropagator {
    pub fn new(name: impl Into<String>, output: FieldId, value: f32) -> Self {
        Self {
            name: name.into(),
            output,
            value,
        }
    }
}

impl Propagator for ConstPropagator {
    fn name(&self) -> &str {
        &self.name
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.output, WriteMode::Full)]
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let output = ctx.writes().write(self.output).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: format!("field {:?} not writable", self.output),
            }
        })?;
        output.fill(self.value);
        Ok(())
    }
}

/// Fails deterministically after a configurable number of successful calls.
///
/// Useful for testing rollback and error propagation in the tick engine.
/// Uses `AtomicUsize` for the call counter so it satisfies `Send`.
pub struct FailingPropagator {
    pub name: String,
    pub output: FieldId,
    pub succeed_count: usize,
    call_count: AtomicUsize,
}

impl FailingPropagator {
    /// Create a propagator that succeeds `succeed_count` times then fails.
    pub fn new(name: impl Into<String>, output: FieldId, succeed_count: usize) -> Self {
        Self {
            name: name.into(),
            output,
            succeed_count,
            call_count: AtomicUsize::new(0),
        }
    }

    /// How many times `step()` has been called.
    pub fn calls(&self) -> usize {
        self.call_count.load(Ordering::Relaxed)
    }

    /// Reset the call counter.
    pub fn reset(&self) {
        self.call_count.store(0, Ordering::Relaxed);
    }
}

impl Propagator for FailingPropagator {
    fn name(&self) -> &str {
        &self.name
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.output, WriteMode::Full)]
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let n = self.call_count.fetch_add(1, Ordering::Relaxed);
        if n >= self.succeed_count {
            return Err(PropagatorError::ExecutionFailed {
                reason: format!(
                    "deliberate failure after {} successful calls",
                    self.succeed_count
                ),
            });
        }
        // On success, fill output with call index for traceability.
        if let Some(output) = ctx.writes().write(self.output) {
            output.fill(n as f32);
        }
        Ok(())
    }
}
