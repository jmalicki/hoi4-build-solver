//! Pluggable heuristics for the HOI4 MDP solver.
//!
//! This module defines the `Heuristic` trait and provides implementations.
//! Heuristics provide both admissible lower bounds (for A*) and upper bounds (for pruning).

mod best_infra_upper_bound;
mod zero;

pub use best_infra_upper_bound::BestInfraUpperBoundHeuristic;
pub use zero::ZeroHeuristic;

use crate::{NodeDesc, State, TargetType};

/// Trait for pluggable heuristics.
///
/// All heuristics must provide:
/// - An admissible lower bound `h(s)` for A* search
/// - An optional upper bound `ub(s)` for pruning
pub trait Heuristic: Send + Sync {
    /// Admissible lower bound on remaining cost from state `st`.
    ///
    /// Must satisfy: h(s) <= actual optimal cost from s to goal
    /// Returns a non-negative value.
    fn lower_bound(
        &self,
        st: &State,
        nodes: &[NodeDesc],
        target_type: TargetType,
        target: i32,
    ) -> f64;

    /// Upper bound on remaining cost from state `st`.
    ///
    /// Used for pruning: if g(s) + ub(s) > best_known_solution_cost,
    /// we can prune state s. Returns f64::INFINITY if no bound is known.
    fn upper_bound(
        &self,
        st: &State,
        nodes: &[NodeDesc],
        target_type: TargetType,
        target: i32,
    ) -> f64;

    /// Human-readable name for this heuristic (for debugging/logging).
    fn name(&self) -> &'static str;
}

/// Create a heuristic by name.
///
/// Returns an error if the name is unknown.
pub fn create_by_name(name: &str) -> Result<Box<dyn Heuristic>, String> {
    match name {
        "best_infra_upper_bound" | "standard" => Ok(Box::new(BestInfraUpperBoundHeuristic)),
        // zero heuristic for Dijkstra's algorithm; accept both spellings
        "djikstra" | "dijkstra" | "zero" => Ok(Box::new(ZeroHeuristic)),
        _ => Err(format!(
            "Unknown heuristic: {}. Available: best_infra_upper_bound (alias: standard), djikstra (alias: dijkstra, zero)",
            name
        )),
    }
}
