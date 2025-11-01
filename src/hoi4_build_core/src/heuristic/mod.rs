//! Pluggable heuristics for the HOI4 build solver.
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
    #[allow(private_interfaces)]
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
    #[allow(private_interfaces)]
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
    match canonical_name(name) {
        Some("best_infra_upper_bound") => Ok(Box::new(BestInfraUpperBoundHeuristic)),
        Some("dijkstra") => Ok(Box::new(ZeroHeuristic)),
        _ => Err(format!(
            "Unknown heuristic: {}. Available: {}",
            name,
            list_names().join(", ")
        )),
    }
}

/// Return canonical heuristic names supported.
pub fn list_names() -> Vec<&'static str> {
    vec!["best_infra_upper_bound", "dijkstra"]
}

/// Map input name (including aliases) to canonical name.
pub fn canonical_name(input: &str) -> Option<&'static str> {
    match input.to_lowercase().as_str() {
        "best_infra_upper_bound" | "standard" => Some("best_infra_upper_bound"),
        "dijkstra" | "zero" => Some("dijkstra"),
        _ => None,
    }
}
