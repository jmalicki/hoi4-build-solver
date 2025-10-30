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
use rapidhash::fast::RandomState as RapidHasher;
use smallvec::SmallVec;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::io::{self, Write};

/// Static descriptor of a node (immutable across search).
#[derive(Clone, Copy)]
struct NodeDesc {
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
#[derive(Clone, Eq, PartialEq, Hash)]
struct State(Vec<NodeState>);

/// Return true if a joint state meets the terminal goal.
///
/// A state is terminal when the total number of military factories across all
/// nodes is greater than or equal to `target`.
fn is_terminal(st: &State, target: i32) -> bool {
    let mut sum = 0i32;
    for ns in &st.0 {
        sum += ns.mil as i32;
    }
    sum >= target
}

/// Admissible heuristic h(s): optimistic lower bound on remaining cost.
///
/// This heuristic matches docs/MODELING.md. It assumes:
/// - Best-case infra multiplier (as if infra=5 on the acting node), and
/// - civUpper = current civilians + max(0, emptySlots - remainingMil) as an
///   upper bound for the denominator effect.
///
/// Returns: non-negative lower bound on the optimal remaining cost from `st`.
fn heuristic(st: &State, nodes: &[NodeDesc], target: i32) -> f64 {
    let mut cur_mil = 0i32;
    let mut sum_civ = 0i32;
    let mut empty = 0i32;
    for (ns, nd) in st.0.iter().zip(nodes.iter()) {
        cur_mil += ns.mil as i32;
        sum_civ += ns.civ as i32;
        let used = ns.civ as i32 + ns.mil as i32;
        empty += ((nd.slots as i32) - used).max(0);
    }
    let remaining = (target - cur_mil).max(0);
    if remaining == 0 {
        return 0.0;
    }
    let civ_upper = (sum_civ + (empty - remaining).max(0)).max(1) as f64;
    let best_mult = 1.0 + (2.0 * 5.0) / 10.0;
    let per_unit = (7200.0f64.min(4000.0)) / best_mult / civ_upper;
    (remaining as f64) * per_unit
}

/// Compute infrastructure multiplier for a given level in [0,5].
fn infra_mult(infra: u8) -> f64 {
    1.0 + (2.0 * (infra as f64)) / 10.0
}

/// Generate feasible successors for a state.
///
/// Yields tuples of (node_index, action_label, next_state, step_cost).
/// The per-step cost uses the pre-action total civilian count as denominator.
fn iter_successors<'a>(
    st: &'a State,
    nodes: &'a [NodeDesc],
) -> impl Iterator<Item = (usize, &'static str, State, f64)> + 'a {
    let civ_den = st.0.iter().map(|ns| ns.civ as i32).sum::<i32>().max(1) as f64;
    st.0.iter().enumerate().flat_map(move |(i, ns)| {
        let nd = &nodes[i];
        let mult = infra_mult(ns.infra);
        let mut out: SmallVec<[(usize, &'static str, State, f64); 4]> = SmallVec::new();
        // civilian
        if (ns.civ + ns.mil) < nd.slots {
            let mut v = st.0.clone();
            v[i].civ += 1;
            out.push((i, "civilian", State(v), 10800.0 / mult / civ_den));
        }
        // military
        if (ns.civ + ns.mil) < nd.slots {
            let mut v = st.0.clone();
            v[i].mil += 1;
            out.push((i, "military", State(v), 7200.0 / mult / civ_den));
        }
        // infra
        if ns.infra < 5 {
            let mut v = st.0.clone();
            v[i].infra += 1;
            out.push((i, "infra", State(v), 6000.0 / mult / civ_den));
        }
        // convert: civ -> mil keeps (civ + mil) unchanged; capacity already ensured elsewhere
        if ns.civ >= 1 {
            let mut v = st.0.clone();
            v[i].civ -= 1;
            v[i].mil += 1;
            out.push((i, "convert", State(v), 4000.0 / mult / civ_den));
        }
        out
    })
}

/// A* binary heap item: (f=g+h, g, tie-counter, state).
#[derive(Clone)]
struct HeapItem(f64, f64, usize, State);
impl PartialEq for HeapItem {
    fn eq(&self, o: &Self) -> bool {
        self.0.eq(&o.0)
    }
}
impl Eq for HeapItem {}
impl PartialOrd for HeapItem {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        o.0.partial_cmp(&self.0)
    }
}
impl Ord for HeapItem {
    fn cmp(&self, o: &Self) -> Ordering {
        self.partial_cmp(o).unwrap_or(Ordering::Equal)
    }
}

/// Solve the problem and reconstruct the plan in one call (Python API).
///
/// Parameters (Python side):
/// - nodes: list of tuples (name, numSlots, numInfra, numCivilian, numMilitary)
/// - target_military: desired total military factories
/// - verbose: print progress lines
/// - print_every: cadence in expansions for progress lines
///
/// Returns (moves, final_state, total_cost):
/// - moves: list of (nodeName, actionLabel)
/// - final_state: list of per-node triples (infra, civ, mil)
/// - total_cost: cost along the optimal plan
/// solve_and_reconstruct(nodes, target_military, *, verbose=True, print_every=1)
///
/// Python entry point. Types:
/// - nodes: list[tuple[str, int, int, int, int]]
/// - target_military: int
/// - verbose: bool (kw-only)
/// - print_every: int (kw-only)
/// Returns tuple[list[(str,str)], list[(int,int,int)], float]
#[pyfunction]
#[pyo3(
    signature = (nodes, target_military, *, verbose=true, print_every=1),
    text_signature = "(nodes: list[tuple[str,int,int,int,int]], target_military: int, *, verbose: bool = True, print_every: int = 1) -> tuple[list[tuple[str,str]], list[tuple[int,int,int]], float]"
)]
fn solve_and_reconstruct(
    _py: Python<'_>,
    nodes: Vec<(String, i32, i32, i32, i32)>,
    target_military: i32,
    verbose: bool,
    print_every: usize,
) -> PyResult<(Vec<(String, String)>, Vec<(i32, i32, i32)>, f64)> {
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
    let mut st_vec: Vec<NodeState> = Vec::with_capacity(nodes.len());
    for t in &nodes {
        let infra = to_u8(_py, t.2, "numInfra", 5)?;
        let civ = to_u8(_py, t.3, "numCivilian", 255)?;
        let mil = to_u8(_py, t.4, "numMilitary", 255)?;
        st_vec.push(NodeState { infra, civ, mil });
    }
    let st = State(st_vec);

    // Feasibility quick check
    let total_slots: i32 = desc.iter().map(|d| d.slots as i32).sum();
    if target_military > total_slots {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "target exceeds capacity",
        ));
    }

    let mut open = BinaryHeap::new();
    let mut counter: usize = 0;
    // g_best: best-known exact cost-so-far g(s) for each discovered state s.
    // Used to avoid revisiting states with worse cost and to compute the final total cost.
    let mut g_best: HashMap<State, f64, RapidHasher> = HashMap::with_hasher(RapidHasher::new());
    // parent: backpointer map for path reconstruction.
    // For a child state s, stores (prev_state, node_index, action_label, step_cost)
    // that achieved the current best g(s). This enables an O(path_len) reverse walk
    // from the goal to the start once the search finishes.
    let mut parent: HashMap<State, (State, usize, &'static str, f64), RapidHasher> =
        HashMap::with_hasher(RapidHasher::new());
    g_best.insert(st.clone(), 0.0);
    let h0 = heuristic(&st, &desc, target_military);
    open.push(HeapItem(h0, 0.0, counter, st.clone()));
    counter += 1;
    let mut expanded: usize = 0;
    let mut goal: Option<State> = None;
    let mut goal_g: f64 = 0.0;
    if verbose {
        println!(
            "[A*] start: heap={} target={} print_every={}",
            open.len(),
            target_military,
            print_every
        );
        let _ = io::stdout().flush();
    }
    while let Some(HeapItem(_f, g, _c, cur)) = open.pop() {
        expanded += 1;
        if verbose && (expanded == 1 || (print_every > 0 && expanded % print_every == 0)) {
            println!("[A*] iters={} g={:.4} heap={}", expanded, g, open.len());
            let _ = io::stdout().flush();
        }
        if is_terminal(&cur, target_military) {
            goal_g = *g_best.get(&cur).unwrap_or(&g);
            goal = Some(cur.clone());
            break;
        }
        if g > *g_best.get(&cur).unwrap_or(&f64::INFINITY) {
            continue;
        }
        for (idx, act, ns, cost) in iter_successors(&cur, &desc) {
            let tentative = g + cost;
            if tentative < *g_best.get(&ns).unwrap_or(&f64::INFINITY) {
                g_best.insert(ns.clone(), tentative);
                parent.insert(ns.clone(), (cur.clone(), idx, act, cost));
                let h = heuristic(&ns, &desc, target_military);
                open.push(HeapItem(tentative + h, tentative, counter, ns));
                counter += 1;
            }
        }
    }
    if let Some(goal_state) = goal {
        // reconstruct once, outside the loop
        let mut moves: Vec<(String, String)> = Vec::new();
        let mut walk = goal_state.clone();
        while let Some((prev, idx, act, _)) = parent.get(&walk).cloned() {
            moves.push((names[idx].clone(), act.to_string()));
            walk = prev;
        }
        moves.reverse();
        if verbose {
            println!(
                "[A*] goal reached: total_cost={:.4} iters={}",
                goal_g, expanded
            );
            let _ = io::stdout().flush();
        }
        let final_state: Vec<(i32, i32, i32)> = goal_state
            .0
            .iter()
            .map(|ns| (ns.infra as i32, ns.civ as i32, ns.mil as i32))
            .collect();
        Ok((moves, final_state, goal_g))
    } else {
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
