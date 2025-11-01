//! Zero heuristic (Dijkstra's algorithm)
//!
//! Always returns 0 for the admissible lower bound and `f64::INFINITY` for the
//! upper bound so no pruning is performed. This reduces A* to Dijkstra's algorithm.

use super::Heuristic;
use crate::{NodeDesc, State, TargetType};

#[derive(Clone, Copy, Debug)]
pub struct ZeroHeuristic;

#[allow(private_interfaces)]
impl Heuristic for ZeroHeuristic {
    fn lower_bound(
        &self,
        _st: &State,
        _nodes: &[NodeDesc],
        _target_type: TargetType,
        _target: i32,
    ) -> f64 {
        0.0
    }

    fn upper_bound(
        &self,
        _st: &State,
        _nodes: &[NodeDesc],
        _target_type: TargetType,
        _target: i32,
    ) -> f64 {
        f64::INFINITY
    }

    fn name(&self) -> &'static str {
        // Use the requested external string name
        "dijkstra"
    }
}
