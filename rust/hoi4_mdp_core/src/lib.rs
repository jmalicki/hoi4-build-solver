// High-performance A* solver for the HOI4 build planning MDP.
//
// This crate implements the core search loop and domain logic in Rust and exposes
// a minimal Python API via PyO3 (in src/py). The Python layer handles CSV/Sheets I/O,
// while Rust manages: state representation, successor generation, heuristic,
// and the A* frontier with a binary heap.
//
// Key modeling choices follow docs/MODELING.md:
// - Deterministic transitions; one node changes per action.
// - Immediate cost per action = base_cost / infra_multiplier / sumCivilian,
//   with the denominator clamped at 1 to avoid division by zero.
// - Heuristic is an admissible lower bound using best-case infra and an upper
//   bound on future civilians: civUpper = civ + max(0, empty - remainingMil).

use smallvec::SmallVec;
use std::cmp::Ordering;

mod heap_growth;
pub mod heuristic;
pub mod core;
mod state_pool;
use state_pool::{StatePool, StateHandle};
use heuristic::{Heuristic, create_by_name};

/// Static descriptor of a node (immutable across search).
#[derive(Clone, Copy)]
pub(crate) struct NodeDesc {
    slots: u8, // 0..=255
}

/// Per-node dynamic state tracked during search.
///
/// - infra: infrastructure level in [0,5]
/// - civ: number of civilian factories (>= 0)
/// - mil: number of military factories (>= 0)
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub(crate) struct NodeState {
    pub(crate) infra: u8, // 0..=5
    pub(crate) civ: u8,   // 0..=255 (see docs)
    pub(crate) mil: u8,   // 0..=255 (see docs)
}

/// Joint state across all nodes.
///
/// Stored as a vector to support variable numbers of nodes; hashing and equality
/// are defined via derives on the inner elements and vector content.
#[derive(Clone, Eq, PartialEq, Hash, Default)]
pub(crate) struct State(pub(crate) Vec<NodeState>);

/// Target type for the MDP goal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetType {
    Military,
    Civilian,
    Factories, // Total factories (military + civilian)
}

/// Return true if a joint state meets the terminal goal.
///
/// The terminal condition depends on the target type:
/// - Military: total military factories >= target
/// - Civilian: total civilian factories >= target
/// - Factories: total factories (military + civilian) >= target
pub(crate) fn is_terminal(st: &State, target_type: TargetType, target: i32) -> bool {
    match target_type {
        TargetType::Military => st.0.iter().map(|ns| ns.mil as i32).sum::<i32>() >= target,
        TargetType::Civilian => st.0.iter().map(|ns| ns.civ as i32).sum::<i32>() >= target,
        TargetType::Factories => st.0.iter().map(|ns| ns.mil as i32 + ns.civ as i32).sum::<i32>() >= target,
    }
}

fn infra_mult(infra: u8) -> f64 {
    1.0 + (2.0 * (infra as f64)) / 10.0
}

fn fmt_count(n: usize) -> String {
    if n >= 1_000_000_000 {
        format!("{:.2}B", (n as f64) / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.2}M", (n as f64) / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.2}K", (n as f64) / 1_000.0)
    } else {
        n.to_string()
    }
}

fn fmt_step(n: usize) -> String {
    if n.is_multiple_of(1_000_000_000) && n >= 1_000_000_000 {
        format!("{}B", n / 1_000_000_000)
    } else if n.is_multiple_of(1_000_000) && n >= 1_000_000 {
        format!("{}M", n / 1_000_000)
    } else if n.is_multiple_of(1_000) && n >= 1_000 {
        format!("{}K", n / 1_000)
    } else {
        n.to_string()
    }
}

/// Information about a successor state generated from an action.
#[derive(Clone)]
struct Successor {
    /// Index of the node that was acted upon
    node_index: usize,
    /// Action label ("civilian", "military", "infra", "convert")
    action: &'static str,
    /// The resulting state after applying the action
    next_state: State,
    /// Step cost for this action (using pre-action total civilian count as denominator)
    step_cost: f64,
}

/// Domain-specific transition information for path reconstruction.
///
/// This is stored in StatePool alongside generic information (parent_idx, component_idx).
/// The pool handles all storage and retrieval - lib.rs doesn't need to manage it.
pub(crate) struct TransitionInfo {
    /// Action label ("civilian", "military", "infra", "convert") - domain-specific
    action: &'static str,
    /// Step cost from parent to this state - domain-specific (could be generic, but stored here for convenience)
    cost: f64,
}

/// Generate feasible successors for a state.
///
/// Yields `Successor` structs containing node_index, action, next_state, and step_cost.
/// The per-step cost uses the pre-action total civilian count as denominator.
pub(crate) fn iter_successors<'a>(
    st: &'a State,
    nodes: &'a [NodeDesc],
) -> impl Iterator<Item = Successor> + 'a {
    let n_nodes = st.0.len();
    let civ_den = st.0.iter().map(|ns| ns.civ as i32).sum::<i32>().max(1) as f64;
    st.0.iter().enumerate().flat_map(move |(i, ns)| {
        let nd = &nodes[i];
        let mult = infra_mult(ns.infra);
        let mut out: SmallVec<[Successor; 4]> = SmallVec::new();
        // civilian
        if (ns.civ + ns.mil) < nd.slots {
            let mut v = Vec::with_capacity(n_nodes);
            v.extend_from_slice(&st.0);
            v[i].civ += 1;
            out.push(Successor { node_index: i, action: "civilian", next_state: State(v), step_cost: 10800.0 / mult / civ_den });
        }
        // military
        if (ns.civ + ns.mil) < nd.slots {
            let mut v = Vec::with_capacity(n_nodes);
            v.extend_from_slice(&st.0);
            v[i].mil += 1;
            out.push(Successor { node_index: i, action: "military", next_state: State(v), step_cost: 7200.0 / mult / civ_den });
        }
        // infra
        if ns.infra < 5 {
            let mut v = Vec::with_capacity(n_nodes);
            v.extend_from_slice(&st.0);
            v[i].infra += 1;
            out.push(Successor { node_index: i, action: "infra", next_state: State(v), step_cost: 6000.0 / mult / civ_den });
        }
        // convert
        if ns.civ >= 1 {
            let mut v = Vec::with_capacity(n_nodes);
            v.extend_from_slice(&st.0);
            v[i].civ -= 1;
            v[i].mil += 1;
            out.push(Successor { node_index: i, action: "convert", next_state: State(v), step_cost: 4000.0 / mult / civ_den });
        }
        out
    })
}
