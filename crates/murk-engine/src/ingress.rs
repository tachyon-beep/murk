//! Bounded ingress queue with deterministic ordering and TTL evaluation.
//!
//! [`IngressQueue`] buffers commands between submission and tick execution.
//! It enforces capacity limits, assigns monotonic arrival sequence numbers,
//! evaluates TTL expiry, and sorts commands into a deterministic order for
//! processing by the tick engine.
//!
//! # Ordering
//!
//! Commands are sorted by the composite key:
//! `(priority_class, source_id|MAX, source_seq|MAX, arrival_seq)`
//!
//! This ensures:
//! - Lower priority class values execute first (0 = system, 1 = user).
//! - Within a priority class, source-keyed commands sort before anonymous ones.
//! - Source-keyed commands from the same source execute in sequence order.
//! - Anonymous commands execute in arrival order.

use std::collections::VecDeque;

use murk_core::command::{Command, Receipt};
use murk_core::error::IngressError;
use murk_core::id::TickId;

/// A command paired with its original batch-local index from `submit()`.
///
/// Returned in [`DrainResult::commands`] so the tick engine can build
/// receipts with correct `command_index` values even after priority reordering.
#[derive(Debug)]
pub struct DrainedCommand {
    /// The command to execute.
    pub command: Command,
    /// The original batch-local index from the `submit()` call.
    pub command_index: usize,
}

/// Result of draining the queue at the start of a tick.
#[derive(Debug)]
pub struct DrainResult {
    /// Commands that passed TTL checks, sorted in deterministic order.
    pub commands: Vec<DrainedCommand>,
    /// Receipts for commands that expired before reaching the current tick.
    pub expired_receipts: Vec<Receipt>,
}

/// A command paired with its original batch-local index.
///
/// Preserves the `command_index` from the `submit()` call so that
/// `drain()` can produce correct receipts for expired commands.
struct QueueEntry {
    command: Command,
    command_index: usize,
}

/// Bounded command queue for the ingress pipeline.
///
/// Accepts batches of commands via [`submit()`](IngressQueue::submit),
/// assigns monotonic arrival sequence numbers, and produces a sorted,
/// TTL-filtered batch via [`drain()`](IngressQueue::drain).
pub struct IngressQueue {
    queue: VecDeque<QueueEntry>,
    capacity: usize,
    next_arrival_seq: u64,
}

impl IngressQueue {
    /// Create a new queue with the given capacity.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "IngressQueue capacity must be at least 1");
        Self {
            queue: VecDeque::with_capacity(capacity),
            capacity,
            next_arrival_seq: 0,
        }
    }

    /// Submit a batch of commands to the queue.
    ///
    /// Returns one [`Receipt`] per input command. Commands are accepted
    /// in order until the queue is full; remaining commands receive
    /// `QueueFull` receipts. If `tick_disabled` is true, all commands
    /// are rejected with `TickDisabled`.
    ///
    /// Arrival sequence numbers are assigned from a monotonic counter
    /// that persists across submit calls, overwriting whatever value
    /// the caller may have set on `Command::arrival_seq`.
    pub fn submit(&mut self, commands: Vec<Command>, tick_disabled: bool) -> Vec<Receipt> {
        let mut receipts = Vec::with_capacity(commands.len());

        for (i, mut cmd) in commands.into_iter().enumerate() {
            if tick_disabled {
                receipts.push(Receipt {
                    accepted: false,
                    applied_tick_id: None,
                    reason_code: Some(IngressError::TickDisabled),
                    command_index: i,
                });
                continue;
            }

            if self.queue.len() >= self.capacity {
                receipts.push(Receipt {
                    accepted: false,
                    applied_tick_id: None,
                    reason_code: Some(IngressError::QueueFull),
                    command_index: i,
                });
                continue;
            }

            cmd.arrival_seq = self.next_arrival_seq;
            self.next_arrival_seq += 1;
            self.queue.push_back(QueueEntry {
                command: cmd,
                command_index: i,
            });

            receipts.push(Receipt {
                accepted: true,
                applied_tick_id: None,
                reason_code: None,
                command_index: i,
            });
        }

        receipts
    }

    /// Drain the queue, filtering expired commands and sorting the rest.
    ///
    /// A command is expired if `cmd.expires_after_tick < current_tick`.
    /// A command with `expires_after_tick == current_tick` is **valid**
    /// during that tick.
    ///
    /// Returns a [`DrainResult`] containing the sorted valid commands
    /// and receipts for expired commands.
    pub fn drain(&mut self, current_tick: TickId) -> DrainResult {
        let mut valid = Vec::new();
        let mut expired_receipts = Vec::new();

        for entry in self.queue.drain(..) {
            if entry.command.expires_after_tick.0 < current_tick.0 {
                expired_receipts.push(Receipt {
                    accepted: true,
                    applied_tick_id: None,
                    reason_code: Some(IngressError::Stale),
                    command_index: entry.command_index,
                });
            } else {
                valid.push(DrainedCommand {
                    command: entry.command,
                    command_index: entry.command_index,
                });
            }
        }

        // Deterministic sort: (priority_class, source_id|MAX, source_seq|MAX, arrival_seq)
        valid.sort_unstable_by_key(|dc| {
            (
                dc.command.priority_class,
                dc.command.source_id.unwrap_or(u64::MAX),
                dc.command.source_seq.unwrap_or(u64::MAX),
                dc.command.arrival_seq,
            )
        });

        DrainResult {
            commands: valid,
            expired_receipts,
        }
    }

    /// Number of commands currently buffered.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Maximum number of commands this queue can hold.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Discard all pending commands.
    ///
    /// Called during [`TickEngine::reset()`](crate::TickEngine::reset) so
    /// stale commands from previous ticks don't survive a reset.
    pub fn clear(&mut self) {
        self.queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::command::CommandPayload;
    use murk_core::id::ParameterKey;

    fn make_cmd(priority: u8, expires: u64) -> Command {
        Command {
            payload: CommandPayload::SetParameter {
                key: ParameterKey(0),
                value: 0.0,
            },
            expires_after_tick: TickId(expires),
            source_id: None,
            source_seq: None,
            priority_class: priority,
            arrival_seq: 0,
        }
    }

    fn make_sourced_cmd(priority: u8, source_id: u64, source_seq: u64, expires: u64) -> Command {
        Command {
            payload: CommandPayload::SetParameter {
                key: ParameterKey(0),
                value: 0.0,
            },
            expires_after_tick: TickId(expires),
            source_id: Some(source_id),
            source_seq: Some(source_seq),
            priority_class: priority,
            arrival_seq: 0,
        }
    }

    // ── submit tests ───────────────────────────────────────────

    #[test]
    fn submit_assigns_monotonic_arrival_seq() {
        let mut q = IngressQueue::new(10);
        let cmds = vec![make_cmd(1, 100), make_cmd(1, 100), make_cmd(1, 100)];
        let receipts = q.submit(cmds, false);
        assert_eq!(receipts.len(), 3);
        assert!(receipts.iter().all(|r| r.accepted));

        // Drain and check arrival_seq 0, 1, 2
        let result = q.drain(TickId(0));
        assert_eq!(result.commands[0].command.arrival_seq, 0);
        assert_eq!(result.commands[1].command.arrival_seq, 1);
        assert_eq!(result.commands[2].command.arrival_seq, 2);
    }

    #[test]
    fn submit_rejects_when_full() {
        let mut q = IngressQueue::new(2);
        let cmds = vec![make_cmd(1, 100), make_cmd(1, 100), make_cmd(1, 100)];
        let receipts = q.submit(cmds, false);
        assert!(receipts[0].accepted);
        assert!(receipts[1].accepted);
        assert!(!receipts[2].accepted);
        assert_eq!(receipts[2].reason_code, Some(IngressError::QueueFull));
    }

    #[test]
    fn submit_rejects_when_tick_disabled() {
        let mut q = IngressQueue::new(10);
        let cmds = vec![make_cmd(1, 100), make_cmd(1, 100)];
        let receipts = q.submit(cmds, true);
        assert!(!receipts[0].accepted);
        assert_eq!(receipts[0].reason_code, Some(IngressError::TickDisabled));
        assert!(!receipts[1].accepted);
        assert_eq!(receipts[1].reason_code, Some(IngressError::TickDisabled));
        assert!(q.is_empty());
    }

    #[test]
    fn submit_partial_accept_on_overflow() {
        let mut q = IngressQueue::new(3);
        let cmds = vec![
            make_cmd(1, 100),
            make_cmd(1, 100),
            make_cmd(1, 100),
            make_cmd(1, 100),
            make_cmd(1, 100),
        ];
        let receipts = q.submit(cmds, false);
        assert_eq!(receipts.len(), 5);
        assert!(receipts[0].accepted);
        assert!(receipts[1].accepted);
        assert!(receipts[2].accepted);
        assert!(!receipts[3].accepted);
        assert_eq!(receipts[3].reason_code, Some(IngressError::QueueFull));
        assert!(!receipts[4].accepted);
        assert_eq!(receipts[4].reason_code, Some(IngressError::QueueFull));
        assert_eq!(q.len(), 3);
    }

    #[test]
    fn arrival_seq_persists_across_submits() {
        let mut q = IngressQueue::new(10);
        q.submit(vec![make_cmd(1, 100), make_cmd(1, 100)], false);
        q.submit(vec![make_cmd(1, 100)], false);
        let result = q.drain(TickId(0));
        assert_eq!(result.commands[0].command.arrival_seq, 0);
        assert_eq!(result.commands[1].command.arrival_seq, 1);
        assert_eq!(result.commands[2].command.arrival_seq, 2);
    }

    #[test]
    fn receipt_command_index_matches_input() {
        let mut q = IngressQueue::new(10);
        let cmds = vec![make_cmd(1, 100), make_cmd(1, 100), make_cmd(1, 100)];
        let receipts = q.submit(cmds, false);
        assert_eq!(receipts[0].command_index, 0);
        assert_eq!(receipts[1].command_index, 1);
        assert_eq!(receipts[2].command_index, 2);
    }

    // ── drain tests ────────────────────────────────────────────

    #[test]
    fn drain_removes_expired_commands() {
        let mut q = IngressQueue::new(10);
        // Expires after tick 3 → expired at tick 4
        q.submit(vec![make_cmd(1, 3)], false);
        let result = q.drain(TickId(4));
        assert!(result.commands.is_empty());
        assert_eq!(result.expired_receipts.len(), 1);
        assert_eq!(
            result.expired_receipts[0].reason_code,
            Some(IngressError::Stale)
        );
    }

    #[test]
    fn drain_keeps_valid_commands() {
        let mut q = IngressQueue::new(10);
        // Expires after tick 10 → valid at tick 5
        q.submit(vec![make_cmd(1, 10)], false);
        let result = q.drain(TickId(5));
        assert_eq!(result.commands.len(), 1);
        assert!(result.expired_receipts.is_empty());
    }

    #[test]
    fn drain_boundary_expires_after_equals_current() {
        let mut q = IngressQueue::new(10);
        // expires_after_tick == 5, drain at tick 5 → VALID (still on that tick)
        q.submit(vec![make_cmd(1, 5)], false);
        let result = q.drain(TickId(5));
        assert_eq!(result.commands.len(), 1);
        assert!(result.expired_receipts.is_empty());
    }

    #[test]
    fn drain_sorts_by_priority() {
        let mut q = IngressQueue::new(10);
        q.submit(
            vec![make_cmd(2, 100), make_cmd(0, 100), make_cmd(1, 100)],
            false,
        );
        let result = q.drain(TickId(0));
        assert_eq!(result.commands[0].command.priority_class, 0);
        assert_eq!(result.commands[1].command.priority_class, 1);
        assert_eq!(result.commands[2].command.priority_class, 2);
    }

    #[test]
    fn drain_sorts_by_source_within_priority() {
        let mut q = IngressQueue::new(10);
        q.submit(
            vec![
                make_sourced_cmd(1, 10, 2, 100),
                make_sourced_cmd(1, 10, 1, 100),
                make_sourced_cmd(1, 5, 0, 100),
            ],
            false,
        );
        let result = q.drain(TickId(0));
        // source_id 5 < 10, so it comes first
        assert_eq!(result.commands[0].command.source_id, Some(5));
        assert_eq!(result.commands[0].command.source_seq, Some(0));
        // Then source_id 10 seq 1 before seq 2
        assert_eq!(result.commands[1].command.source_id, Some(10));
        assert_eq!(result.commands[1].command.source_seq, Some(1));
        assert_eq!(result.commands[2].command.source_id, Some(10));
        assert_eq!(result.commands[2].command.source_seq, Some(2));
    }

    #[test]
    fn drain_sorts_by_arrival_seq_when_no_source() {
        let mut q = IngressQueue::new(10);
        // All same priority, no source → sorted by arrival_seq
        q.submit(
            vec![make_cmd(1, 100), make_cmd(1, 100), make_cmd(1, 100)],
            false,
        );
        let result = q.drain(TickId(0));
        assert_eq!(result.commands[0].command.arrival_seq, 0);
        assert_eq!(result.commands[1].command.arrival_seq, 1);
        assert_eq!(result.commands[2].command.arrival_seq, 2);
    }

    #[test]
    fn drain_mixed_source_and_no_source() {
        let mut q = IngressQueue::new(10);
        q.submit(
            vec![
                make_cmd(1, 100),               // no source → source_id=MAX
                make_sourced_cmd(1, 5, 0, 100), // source_id=5
            ],
            false,
        );
        let result = q.drain(TickId(0));
        // source_id 5 < u64::MAX, so sourced command comes first
        assert_eq!(result.commands[0].command.source_id, Some(5));
        assert_eq!(result.commands[1].command.source_id, None);
    }

    #[test]
    fn drain_empty_queue() {
        let mut q = IngressQueue::new(10);
        let result = q.drain(TickId(0));
        assert!(result.commands.is_empty());
        assert!(result.expired_receipts.is_empty());
    }

    #[test]
    fn drain_all_expired() {
        let mut q = IngressQueue::new(10);
        q.submit(vec![make_cmd(1, 0), make_cmd(1, 1), make_cmd(1, 2)], false);
        let result = q.drain(TickId(10));
        assert!(result.commands.is_empty());
        assert_eq!(result.expired_receipts.len(), 3);
    }

    #[test]
    fn drain_expired_receipts_preserve_command_index() {
        let mut q = IngressQueue::new(10);
        // Submit a batch of 4 commands; the middle two will expire.
        q.submit(
            vec![
                make_cmd(1, 100), // index 0 — valid
                make_cmd(1, 2),   // index 1 — expires at tick 3
                make_cmd(1, 1),   // index 2 — expires at tick 3
                make_cmd(1, 100), // index 3 — valid
            ],
            false,
        );
        let result = q.drain(TickId(3));
        assert_eq!(result.commands.len(), 2);
        assert_eq!(result.expired_receipts.len(), 2);
        // Expired receipts carry their original batch indices, not 0.
        assert_eq!(result.expired_receipts[0].command_index, 1);
        assert_eq!(result.expired_receipts[1].command_index, 2);
    }

    #[test]
    fn drain_expired_receipts_across_batches() {
        let mut q = IngressQueue::new(10);
        // Batch 1: indices 0, 1
        q.submit(vec![make_cmd(1, 0), make_cmd(1, 100)], false);
        // Batch 2: indices 0, 1 (new batch, indices reset)
        q.submit(vec![make_cmd(1, 100), make_cmd(1, 0)], false);
        let result = q.drain(TickId(5));
        assert_eq!(result.commands.len(), 2);
        assert_eq!(result.expired_receipts.len(), 2);
        // First expired came from batch 1 index 0
        assert_eq!(result.expired_receipts[0].command_index, 0);
        // Second expired came from batch 2 index 1
        assert_eq!(result.expired_receipts[1].command_index, 1);
    }

    #[test]
    fn drain_clears_queue() {
        let mut q = IngressQueue::new(10);
        q.submit(vec![make_cmd(1, 100), make_cmd(1, 100)], false);
        assert_eq!(q.len(), 2);
        let _ = q.drain(TickId(0));
        assert!(q.is_empty());
    }

    // ── proptest ───────────────────────────────────────────────

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_command() -> impl Strategy<Value = Command> {
            (
                0u8..4,
                any::<u64>(),
                prop::option::of(0u64..100),
                prop::option::of(0u64..100),
            )
                .prop_map(|(prio, expires, src_id, src_seq)| Command {
                    payload: CommandPayload::SetParameter {
                        key: ParameterKey(0),
                        value: 0.0,
                    },
                    expires_after_tick: TickId(expires),
                    source_id: src_id,
                    source_seq: src_seq,
                    priority_class: prio,
                    arrival_seq: 0,
                })
        }

        proptest! {
            #[test]
            fn drain_always_sorted(commands in prop::collection::vec(arb_command(), 0..64)) {
                let mut q = IngressQueue::new(128);
                q.submit(commands, false);
                let result = q.drain(TickId(0));

                // Verify sort order
                for window in result.commands.windows(2) {
                    let a = &window[0].command;
                    let b = &window[1].command;
                    let key_a = (
                        a.priority_class,
                        a.source_id.unwrap_or(u64::MAX),
                        a.source_seq.unwrap_or(u64::MAX),
                        a.arrival_seq,
                    );
                    let key_b = (
                        b.priority_class,
                        b.source_id.unwrap_or(u64::MAX),
                        b.source_seq.unwrap_or(u64::MAX),
                        b.arrival_seq,
                    );
                    prop_assert!(key_a <= key_b, "sort violated: {key_a:?} > {key_b:?}");
                }
            }
        }
    }
}
