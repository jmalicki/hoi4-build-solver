use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::core;
use crate::{NodeDesc, State, TargetType};

// Custom Python exception to signal user-requested early stop
pyo3::create_exception!(hoi4_build_core, SearchStoppedError, PyException);

/// Snapshot of solver progress, exposed to Python as a read-only class.
#[pyclass]
#[derive(Clone)]
struct PyProgressSnapshot {
    #[pyo3(get)]
    iterations: usize,
    #[pyo3(get)]
    cost_from_start: f64,
    #[pyo3(get)]
    heap_size: usize,
    #[pyo3(get)]
    total_states: usize,
    #[pyo3(get)]
    avg_f: f64,
    #[pyo3(get)]
    pruned: usize,
    #[pyo3(get)]
    best_upper_bound: f64,
}

/// Result of solver execution, exposed to Python as a read-only class.
#[pyclass]
#[derive(Clone)]
struct PySolverResult {
    #[pyo3(get)]
    moves: Vec<(String, String)>,
    #[pyo3(get)]
    final_state: Vec<(i32, i32, i32)>,
    #[pyo3(get)]
    total_cost: f64,
}

#[pyfunction]
#[pyo3(
    signature = (nodes, target_type, target, *, print_every=1, prune=false, heuristic="best_infra_upper_bound", progress_callback=None),
    text_signature = "(nodes: list[tuple[str,int,int,int,int]], target_type: str, target: int, *, print_every: int = 1, prune: bool = False, heuristic: str = 'best_infra_upper_bound', progress_callback: Optional[Callable[[ProgressSnapshot], bool]] = None) -> SolverResult"
)]
fn solve_and_reconstruct(
    _py: Python<'_>,
    nodes: Vec<(String, i32, i32, i32, i32)>,
    target_type: String,
    target: i32,
    print_every: usize,
    prune: bool,
    heuristic: &str,
    progress_callback: Option<Bound<PyAny>>,
) -> PyResult<PySolverResult> {
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
    let mut st_vec: Vec<crate::NodeState> = Vec::with_capacity(n_nodes);
    for t in &nodes {
        let infra = to_u8(_py, t.2, "numInfra", 5)?;
        let civ = to_u8(_py, t.3, "numCivilian", 255)?;
        let mil = to_u8(_py, t.4, "numMilitary", 255)?;
        st_vec.push(crate::NodeState { infra, civ, mil });
    }
    let st = State(st_vec);

    // Feasibility quick check
    let total_slots: i32 = desc.iter().map(|d| d.slots as i32).sum();
    if target > total_slots {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "target exceeds capacity",
        ));
    }

    // Build core options and callback
    let mut core_cb = progress_callback.map(|cb| {
        move |snap: &core::ProgressSnapshot| -> bool {
            let py_snap = PyProgressSnapshot {
                iterations: snap.iterations,
                cost_from_start: snap.cost_from_start,
                heap_size: snap.heap_size,
                total_states: snap.total_states,
                avg_f: snap.avg_f,
                pruned: snap.pruned,
                best_upper_bound: snap.best_upper_bound,
            };
            Python::with_gil(|_py| match cb.call1((py_snap,)) {
                Ok(val) => val.extract::<bool>().unwrap_or(false),
                Err(_) => false,
            })
        }
    });
    let opts = core::SolveOptions {
        prune,
        print_every,
        heuristic_name: heuristic,
        progress_cb: core_cb.as_mut(),
    };
    let (moves_idx, final_state_rs, total_cost) =
        core::solve_and_reconstruct_core(desc, st.clone(), target_type_enum, target, opts);
    let final_state: Vec<(i32, i32, i32)> = final_state_rs
        .0
        .iter()
        .map(|ns| (ns.infra as i32, ns.civ as i32, ns.mil as i32))
        .collect();
    let moves: Vec<(String, String)> = moves_idx
        .into_iter()
        .map(|(i, action)| (names[i].clone(), action.to_string()))
        .collect();
    Ok(PySolverResult {
        moves,
        final_state,
        total_cost,
    })
}

#[pymodule]
fn hoi4_build_core(_py: Python, m: &Bound<PyModule>) -> PyResult<()> {
    m.setattr(
        "__doc__",
        "Rust A* core for HOI4 build solver: solve_and_reconstruct(nodes, target, *, print_every, prune, heuristic)",
    )?;
    m.add_function(wrap_pyfunction!(solve_and_reconstruct, m)?)?;
    m.add_class::<PySolverResult>()?;
    m.add_class::<PyProgressSnapshot>()?;
    m.add("SearchStoppedError", _py.get_type::<SearchStoppedError>())?;
    Ok(())
}
