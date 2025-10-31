// High-performance A* solver for the HOI4 build planning MDP.
//
// This crate implements the core search loop and domain logic in Rust and exposes
// a minimal Python API via PyO3. The Python layer handles CSV/Sheets I/O,
// while Rust manages: state representation, successor generation, heuristic,
// and the A* frontier with a binary heap.
//
// Key modeling choices follow docs/MODELING.md:
// - Deterministic transitions; one node changes per action.
// - Immediate cost per action = base_cost / infra_multiplier / sumCivilian,
//   with the denominator clamped at 1 to avoid division by zero.
// - Heuristic is an admissible lower bound using best-case infra and an upper
//   bound on future civilians: civUpper = civ + max(0, empty - remainingMil).
//
use pyo3::prelude::*;
use smallvec::SmallVec;
use std::cmp::Ordering;
use std::io::{self, Write};

mod heap_growth;
mod heuristic;
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
struct NodeState {
    infra: u8, // 0..=5
    civ: u8,   // 0..=255 (see docs)
    mil: u8,   // 0..=255 (see docs)
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
fn is_terminal(st: &State, target_type: TargetType, target: i32) -> bool {
    match target_type {
        TargetType::Military => {
            let mut sum = 0i32;
            for ns in &st.0 {
                sum += ns.mil as i32;
            }
            sum >= target
        }
        TargetType::Civilian => {
            let mut sum = 0i32;
            for ns in &st.0 {
                sum += ns.civ as i32;
            }
            sum >= target
        }
        TargetType::Factories => {
            let mut sum = 0i32;
            for ns in &st.0 {
                sum += ns.mil as i32 + ns.civ as i32;
            }
            sum >= target
        }
    }
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
struct TransitionInfo {
    /// Action label ("civilian", "military", "infra", "convert") - domain-specific
    action: &'static str,
    /// Step cost from parent to this state - domain-specific (could be generic, but stored here for convenience)
    cost: f64,
}

/// Generate feasible successors for a state.
///
/// Yields `Successor` structs containing node_index, action, next_state, and step_cost.
/// The per-step cost uses the pre-action total civilian count as denominator.
fn iter_successors<'a>(
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
            out.push(Successor {
                node_index: i,
                action: "civilian",
                next_state: State(v),
                step_cost: 10800.0 / mult / civ_den,
            });
        }
        // military
        if (ns.civ + ns.mil) < nd.slots {
            let mut v = Vec::with_capacity(n_nodes);
            v.extend_from_slice(&st.0);
            v[i].mil += 1;
            out.push(Successor {
                node_index: i,
                action: "military",
                next_state: State(v),
                step_cost: 7200.0 / mult / civ_den,
            });
        }
        // infra
        if ns.infra < 5 {
            let mut v = Vec::with_capacity(n_nodes);
            v.extend_from_slice(&st.0);
            v[i].infra += 1;
            out.push(Successor {
                node_index: i,
                action: "infra",
                next_state: State(v),
                step_cost: 6000.0 / mult / civ_den,
            });
        }
        // convert: civ -> mil keeps (civ + mil) unchanged; capacity already ensured elsewhere
        if ns.civ >= 1 {
            let mut v = Vec::with_capacity(n_nodes);
            v.extend_from_slice(&st.0);
            v[i].civ -= 1;
            v[i].mil += 1;
            out.push(Successor {
                node_index: i,
                action: "convert",
                next_state: State(v),
                step_cost: 4000.0 / mult / civ_den,
            });
        }
        out
    })
}

/// Solve the problem and reconstruct the plan in one call (Python API).
///
/// Parameters (Python side):
/// - nodes: list of tuples (name, numSlots, numInfra, numCivilian, numMilitary)
/// - target_type: "military", "civilian", or "factories"
/// - target: desired target value for the selected type
/// - verbose: print progress lines
/// - print_every: cadence in expansions for progress lines
///
/// Returns (moves, final_state, total_cost):
/// - moves: list of (nodeName, actionLabel)
/// - final_state: list of per-node triples (infra, civ, mil)
/// - total_cost: cost along the optimal plan
/// solve_and_reconstruct(nodes, target_type, target, *, verbose=True, print_every=1)
///
/// Python entry point. Types:
/// - nodes: list[tuple[str, int, int, int, int]]
/// - target_type: str ("military", "civilian", or "factories")
/// - target: int
/// - verbose: bool (kw-only)
/// - print_every: int (kw-only)
/// - prune: bool (kw-only)
/// - heuristic: str (kw-only, heuristic name)
/// Returns tuple[list[(str,str)], list[(int,int,int)], float]
#[pyfunction]
#[pyo3(
    signature = (nodes, target_type, target, *, verbose=false, print_every=1, prune=false, heuristic="best_infra_upper_bound"),
    text_signature = "(nodes: list[tuple[str,int,int,int,int]], target_type: str, target: int, *, verbose: bool = False, print_every: int = 1, prune: bool = False, heuristic: str = 'best_infra_upper_bound') -> tuple[list[tuple[str,str]], list[tuple[int,int,int]], float]"
)]
fn solve_and_reconstruct(
    _py: Python<'_>,
    nodes: Vec<(String, i32, i32, i32, i32)>,
    target_type: String,
    target: i32,
    verbose: bool,
    print_every: usize,
    prune: bool,
    heuristic: String,
) -> PyResult<(Vec<(String, String)>, Vec<(i32, i32, i32)>, f64)> {
    // Parse target type
    let target_type_enum = match target_type.to_lowercase().as_str() {
        "military" => TargetType::Military,
        "civilian" => TargetType::Civilian,
        "factories" => TargetType::Factories,
        _ => {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Invalid target_type: {}. Must be 'military', 'civilian', or 'factories'",
                target_type
            )));
        }
    };
    fn to_u8(_py: Python<'_>, v: i32, field: &str, max: u8) -> PyResult<u8> {
        if v < 0 || v as u32 > max as u32 {
            Err(pyo3::exceptions::PyValueError::new_err(format!(
                "{} out of range (0..={})",
                field, max
            )))
        } else {
            Ok(v as u8)
        }
    }
    let names: Vec<String> = nodes.iter().map(|t| t.0.clone()).collect();
    let mut desc: Vec<NodeDesc> = Vec::with_capacity(nodes.len());
    for t in &nodes {
        let slots = to_u8(_py, t.1, "numSlots", 255)?;
        desc.push(NodeDesc { slots });
    }
    let n_nodes = nodes.len();
    let mut st_vec: Vec<NodeState> = Vec::with_capacity(n_nodes);
    for t in &nodes {
        let infra = to_u8(_py, t.2, "numInfra", 5)?;
        let civ = to_u8(_py, t.3, "numCivilian", 255)?;
        let mil = to_u8(_py, t.4, "numMilitary", 255)?;
        st_vec.push(NodeState { infra, civ, mil });
    }
    // st_vec is already exactly sized (capacity = len) since we used with_capacity and pushed exactly n_nodes
    let st = State(st_vec);

    // Feasibility quick check
    let total_slots: i32 = desc.iter().map(|d| d.slots as i32).sum();
    if target > total_slots {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "target exceeds capacity",
        ));
    }

    // Create heuristic by name
    let heuristic_impl = create_by_name(&heuristic).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Invalid heuristic: {}", e))
    })?;

    // State pool for managing states, reference counting, index reuse, and the heap
    let initial_heap_bound = 10_000_000; // Start with 10M, will grow as needed
    let mut pool = StatePool::<State, TransitionInfo>::new(initial_heap_bound);

    // seed start state - enqueue normally (no parent)
    let h0 = heuristic_impl.lower_bound(&st, &desc, target_type_enum, target);
    pool.enqueue_or_update_state(st.clone(), 0.0, None, 0, None, h0);
    // Global best known solution cost (upper bound). Initialize with greedy plan from start.
    let mut best_ub = heuristic_impl.upper_bound(&st, &desc, target_type_enum, target);
    let mut expanded: usize = 0;
    let mut goal_i: Option<StateHandle<State, TransitionInfo>> = None;
    let mut goal_g: f64 = 0.0;
    let mut pruned: usize = 0;
    if verbose {
        let pe = fmt_step(print_every);
        println!(
            "[A*] start: heap={} target_type={:?} target={} print_every={} heuristic={}",
            pool.heap_size(),
            target_type_enum,
            target,
            pe,
            heuristic_impl.name()
        );
        let _ = io::stdout().flush();
    }
    while let Some(cur_handle) = pool.heap_pop() {
        // StateHandle guarantees the state is active - if we have a handle, it's valid.
        // The handle already has a ref_count (created when popped from heap).
        // When handle drops, it will automatically decrement ref_count.
        expanded += 1;

        let cur_cost = cur_handle.cost_from_start(&pool);
        let cur = cur_handle.state(&pool).unwrap();
        // Check terminal before any pruning
        if is_terminal(cur, target_type_enum, target) {
            goal_g = cur_cost;
            // Store handle to keep goal state alive for path reconstruction
            // Handle will keep ref_count > 0 until we drop it
            goal_i = Some(cur_handle);
            break;
        }
        if verbose && (expanded == 1 || (print_every > 0 && expanded.is_multiple_of(print_every))) {
            let heap_avg_f = pool.heap_avg_f();
            let iters_pretty = fmt_count(expanded);
            let heap_pretty = fmt_count(pool.heap_size());
            let states_pretty = fmt_count(pool.total_states());
            let pruned_pretty = fmt_count(pruned);
            let msg = format!(
                "[A*] iters={} cost={:.4} heap={} states={} avg_f={:.4} pruned={}",
                iters_pretty, cur_cost, heap_pretty, states_pretty, heap_avg_f, pruned_pretty
            );
            println!("{}", msg);
            let _ = io::stdout().flush();
        }
        if prune {
            // Anytime pruning: tighten UB opportunistically, then prune.
            // We rerun this here in case we've observed a better upper bound since
            // we first enqueued the state.
            let ub_suffix = heuristic_impl.upper_bound(cur, &desc, target_type_enum, target);
            let candidate_total = cur_cost + ub_suffix;
            if candidate_total <= best_ub {
                best_ub = candidate_total;
            } else {
                continue;
            }
        }
        let successors = iter_successors(cur, &desc).collect::<Vec<_>>();
        for successor in successors {
            let cost_value = cur_cost + successor.step_cost;

            // Compute heuristic for the successor state
            let h = heuristic_impl.lower_bound(&successor.next_state, &desc, target_type_enum, target);
            let f = cost_value + h;

            if prune {
                // prune neighbors exceeding current upper bound before enqueueing
                let ub_ns = heuristic_impl.upper_bound(
                    &successor.next_state,
                    &desc,
                    target_type_enum,
                    target,
                );
                if cost_value + ub_ns > best_ub {
                    pruned += 1;
                    continue;
                } else {
                    best_ub = cost_value + ub_ns;
                }
            }

            // Fire-and-forget: pool handles best cost comparison, parent updates,
            // heap operations (decrease_key vs push), and heap growth
            // If state already has better cost_from_start value, it's skipped internally
            // Transition info is stored in the pool - no separate tracking needed
            pool.enqueue_or_update_state(
                successor.next_state,
                cost_value,
                Some(&cur_handle),
                successor.node_index,
                Some(TransitionInfo {
                    action: successor.action,
                    cost: successor.step_cost,
                }),
                f,
            );
        }
        // After processing all successors, parent references have been set (which incremented ref_count).
        // When cur_handle drops, it will automatically decrement ref_count.
        // If the state has children, their parent references will keep it alive (ref_count > 0).
        // If it has no children, decrementing will free it, which is correct.
        // cur_handle is dropped here at end of loop iteration
    }
    if verbose {
        let heap_avg_f = pool.heap_avg_f();
        let iters_pretty = fmt_count(expanded);
        let heap_pretty = fmt_count(pool.heap_size());
        let states_pretty = fmt_count(pool.total_states());
        let pruned_pretty = fmt_count(pruned);
        // Reuse cadence format; use goal_g if available, else 0.0. Show best_ub as candidate_total.
        let final_g = goal_i.is_some().then(|| goal_g).unwrap_or(0.0);
        let msg = format!(
            "[A*] iters={} cost={:.4} heap={} states={} avg_f={:.4} pruned={} ub: best_ub={:.4}",
            iters_pretty, final_g, heap_pretty, states_pretty, heap_avg_f, pruned_pretty, best_ub,
        );
        println!("{}", msg);
        let _ = io::stdout().flush();
    }
    if let Some(goal_handle) = goal_i {
        // Extract final state before path reconstruction (since goal_handle will be moved)
        // Clone NodeState values for better ergonomics, then convert to tuples for Python
        let final_state: Vec<(i32, i32, i32)> = goal_handle
            .state(&pool)
            .unwrap()
            .0
            .iter()
            .map(|ns| {
                let ns = *ns; // Clone NodeState (Copy)
                (ns.infra as i32, ns.civ as i32, ns.mil as i32)
            })
            .collect();
        
        // Reconstruct path backwards from goal to start.
        // All states in the path are kept alive by:
        // - Goal state: ref_count incremented when handle was created
        // - Path states: Each state's parent points to its parent, keeping parents alive
        //   (parent ref_count incremented when parent is set via pool.set_parent_component_and_transition)
        let mut moves: Vec<(String, String)> = Vec::new();
        let mut walk = goal_handle;
        while let Some(parent_handle) = walk.parent(&mut pool) {
            let component_idx = walk.component_index(&pool).unwrap_or(0);
            let action = walk.transition_info(&pool)
                .map(|t| t.action)
                .unwrap_or("unknown");
            moves.push((names[component_idx].clone(), action.to_string()));
            walk = parent_handle;
        }
        moves.reverse();
        if verbose {
            let iters_pretty = fmt_count(expanded);
            println!(
                "[A*] goal reached: total_cost={:.4} iters={}",
                goal_g, iters_pretty
            );
            let _ = io::stdout().flush();
        }
        Ok((moves, final_state, goal_g))
    } else {
        if verbose {
            let heap_avg_f = pool.heap_avg_f();
            let iters_pretty = fmt_count(expanded);
            let heap_pretty = fmt_count(pool.heap_size());
            let states_pretty = fmt_count(pool.total_states());
            let pruned_pretty = fmt_count(pruned);
            let msg = format!(
            "[A*] final_iters={} cost={:.4} heap={} states={} avg_f={:.4} pruned={} ub: best_ub={:.4}",
            iters_pretty, 0.0, heap_pretty, states_pretty, heap_avg_f, pruned_pretty, best_ub,
            );
            println!("{}", msg);
            let _ = io::stdout().flush();
        }
        Err(pyo3::exceptions::PyRuntimeError::new_err(
            "A* exhausted without finding a goal",
        ))
    }
}

/// Python module initializer for `hoi4_mdp_core`.
#[pymodule]
fn hoi4_mdp_core(_py: Python, m: &Bound<PyModule>) -> PyResult<()> {
    // Module docstring
    m.setattr(
        "__doc__",
        "Rust A* core for HOI4 MDP: solve_and_reconstruct(nodes, target_military, *, verbose=True, print_every=1)",
    )?;
    m.add_function(wrap_pyfunction!(solve_and_reconstruct, m)?)?;
    Ok(())
}
// TEST REBUILD 1761866152
