//! Exact solver for very small instances using Dijkstra's algorithm.
//!
//! This module provides an exact optimal cost calculator for tiny instances
//! (≤2 nodes, ≤3 slots, ≤2 target) to validate heuristic admissibility.
//! It uses Dijkstra's algorithm with state memoization to find the optimal solution.

use crate::{NodeDesc, State, TargetType, is_terminal};
use std::collections::{BinaryHeap, HashMap};

use std::cmp::Ordering;

// Wrapper for Dijkstra priority queue (min-heap by cost)
#[derive(Clone)]
struct StateCost {
    state: State,
    cost: f64,
}

impl Eq for StateCost {}

impl PartialEq for StateCost {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost && self.state == other.state
    }
}

impl PartialOrd for StateCost {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // Reverse order for min-heap (lowest cost first)
        other.cost.partial_cmp(&self.cost)
    }
}

impl Ord for StateCost {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse order for min-heap (lowest cost first)
        // Use partial_cmp which handles f64 comparison
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}

/// Maximum bounds for exact solver (to keep runtime reasonable).
const MAX_NODES: usize = 2;
const MAX_SLOTS: u8 = 3;
const MAX_TARGET: i32 = 2;
const MAX_STATES: usize = 10_000;

/// Compute exact optimal cost for a tiny instance using Dijkstra's algorithm.
///
/// Returns `Some(cost)` if optimal solution found, `None` if instance is too large
/// or no solution exists.
///
/// This solver is only intended for very small instances (≤2 nodes, ≤3 slots, ≤2 target)
/// to validate heuristic admissibility. For larger instances, it will return `None`.
#[allow(private_interfaces)]
pub fn exact_optimal_cost(
    desc: &[NodeDesc],
    start: &State,
    target_type: TargetType,
    target: i32,
) -> Option<f64> {
    // Check bounds: only solve if instance is small enough
    if desc.len() > MAX_NODES {
        return None;
    }
    if desc.iter().any(|d| d.slots > MAX_SLOTS) {
        return None;
    }
    if target > MAX_TARGET {
        return None;
    }

    // Check if already at target
    if is_terminal(start, target_type, target) {
        return Some(0.0);
    }

    // Dijkstra's algorithm with state memoization (priority queue by cost)
    let mut visited: HashMap<State, f64> = HashMap::new();
    let mut queue: BinaryHeap<StateCost> = BinaryHeap::new();

    visited.insert(start.clone(), 0.0);
    queue.push(StateCost {
        state: start.clone(),
        cost: 0.0,
    });

    let mut states_explored = 0usize;

    while let Some(StateCost {
        state: current_state,
        cost,
    }) = queue.pop()
    {
        states_explored += 1;

        // Safety check: abort if too many states
        if states_explored > MAX_STATES {
            return None;
        }

        // Check if we've reached the target
        if is_terminal(&current_state, target_type, target) {
            return Some(cost);
        }

        // Generate and explore successors
        for successor in crate::iter_successors(&current_state, desc) {
            let new_cost = cost + successor.step_cost;

            // Check if we've seen this state with a better or equal cost
            match visited.get(&successor.next_state) {
                Some(&existing_cost) if existing_cost <= new_cost => {
                    // Already found a path to this state with equal or lower cost
                    continue;
                }
                _ => {
                    // New state or found a better path to this state
                    visited.insert(successor.next_state.clone(), new_cost);
                    queue.push(StateCost {
                        state: successor.next_state.clone(),
                        cost: new_cost,
                    });
                }
            }
        }
    }

    // No solution found (shouldn't happen for valid instances, but possible if bounds violated)
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeState;

    fn make_tiny_desc() -> Vec<NodeDesc> {
        vec![NodeDesc { slots: 2 }, NodeDesc { slots: 1 }]
    }

    fn make_tiny_start() -> State {
        State(vec![
            NodeState {
                infra: 0,
                civ: 1,
                mil: 0,
            },
            NodeState {
                infra: 0,
                civ: 0,
                mil: 0,
            },
        ])
    }

    #[test]
    fn test_already_at_target() {
        // Test case where start state already satisfies target
        let desc = vec![NodeDesc { slots: 2 }];
        let start = State(vec![NodeState {
            infra: 0,
            civ: 0,
            mil: 2,
        }]);
        let cost = exact_optimal_cost(&desc, &start, TargetType::Military, 2);
        assert_eq!(cost, Some(0.0));
    }

    #[test]
    fn test_too_large_instance() {
        // Test that solver rejects instances that are too large
        let desc = vec![
            NodeDesc { slots: 1 },
            NodeDesc { slots: 1 },
            NodeDesc { slots: 1 },
        ];
        let start = State(vec![
            NodeState {
                infra: 0,
                civ: 0,
                mil: 0,
            },
            NodeState {
                infra: 0,
                civ: 0,
                mil: 0,
            },
            NodeState {
                infra: 0,
                civ: 0,
                mil: 0,
            },
        ]);
        let cost = exact_optimal_cost(&desc, &start, TargetType::Military, 1);
        assert_eq!(cost, None);
    }

    #[test]
    fn test_simple_one_node_one_military() {
        // Test: 1 node, need 1 military factory, start with 0
        // Should build 1 military factory
        let desc = vec![NodeDesc { slots: 1 }];
        let start = State(vec![NodeState {
            infra: 0,
            civ: 0,
            mil: 0,
        }]);
        let cost = exact_optimal_cost(&desc, &start, TargetType::Military, 1);
        assert!(cost.is_some(), "Should find solution");
        if let Some(c) = cost {
            // Cost should be 7200 / 1.0 / 1.0 = 7200.0 (no infra, no civilians)
            assert!((c - 7200.0).abs() < 1e-9, "Expected cost ~7200, got {}", c);
        }
    }

    #[test]
    fn test_simple_convert_case() {
        // Test: 1 node with 1 civilian, need 1 military
        // Optimal: convert civilian to military (4000 base cost)
        let desc = vec![NodeDesc { slots: 2 }];
        let start = State(vec![NodeState {
            infra: 0,
            civ: 1,
            mil: 0,
        }]);
        let cost = exact_optimal_cost(&desc, &start, TargetType::Military, 1);
        assert!(cost.is_some(), "Should find solution");
        if let Some(c) = cost {
            // Cost should be 4000 / 1.0 / 1.0 = 4000.0
            assert!((c - 4000.0).abs() < 1e-9, "Expected cost ~4000, got {}", c);
        }
    }

    #[test]
    fn test_failing_test_case() {
        // Test the exact failing test case from prune_does_not_expand_more_than_no_prune_and_cost_matches
        let desc = make_tiny_desc();
        let start = make_tiny_start();
        let cost = exact_optimal_cost(&desc, &start, TargetType::Military, 2);
        assert!(
            cost.is_some(),
            "Should find solution for this small instance"
        );
        eprintln!("Exact optimal cost for failing test case: {:?}", cost);
    }
}
