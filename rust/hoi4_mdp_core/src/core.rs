use crate::heuristic::{Heuristic, create_by_name};
use crate::state_pool::{StatePool, StateHandle};
use crate::{State, NodeDesc, TargetType, is_terminal, iter_successors};

#[derive(Clone)]
pub struct ProgressSnapshot {
    pub iterations: usize,
    pub cost_from_start: f64,
    pub heap_size: usize,
    pub total_states: usize,
    pub avg_f: f64,
    pub pruned: usize,
    pub best_upper_bound: f64,
}

pub struct SolveOptions<'a, F: FnMut(&ProgressSnapshot) -> bool> {
    pub prune: bool,
    pub print_every: usize,
    pub heuristic_name: &'a str,
    pub progress_cb: Option<F>,
}

pub fn solve_and_reconstruct_core<F>(
    desc: Vec<NodeDesc>,
    start: State,
    target_type: TargetType,
    target: i32,
    mut opts: SolveOptions<'_, F>,
) -> (Vec<(usize, &'static str)>, State, f64)
where
    F: FnMut(&ProgressSnapshot) -> bool,
{
    let heuristic_impl = create_by_name(opts.heuristic_name).expect("heuristic must be valid");
    let mut pool = StatePool::<State, super::TransitionInfo>::new(10_000_000);
    let h0 = heuristic_impl.lower_bound(&start, &desc, target_type, target);
    pool.enqueue_or_update_state(start.clone(), 0.0, None, 0, None, h0);
    let mut best_ub = heuristic_impl.upper_bound(&start, &desc, target_type, target);
    let mut expanded: usize = 0;
    let mut goal_i: Option<StateHandle<State, super::TransitionInfo>> = None;
    let mut goal_g: f64 = 0.0;
    let mut pruned: usize = 0;

    while let Some(cur_handle) = pool.heap_pop() {
        expanded += 1;

        let cur_cost = cur_handle.cost_from_start(&pool);
        let cur = cur_handle.state(&pool).unwrap();
        if is_terminal(cur, target_type, target) {
            goal_g = cur_cost;
            goal_i = Some(cur_handle);
            break;
        }
        if let Some(cb) = opts.progress_cb.as_mut() {
            if opts.print_every > 0 && (expanded == 1 || expanded % opts.print_every == 0) {
                let snap = ProgressSnapshot {
                    iterations: expanded,
                    cost_from_start: cur_cost,
                    heap_size: pool.heap_size(),
                    total_states: pool.total_states(),
                    avg_f: pool.heap_avg_f(),
                    pruned,
                    best_upper_bound: best_ub,
                };
                if cb(&snap) {
                    // Early stop: return current frontier head path as best-effort using current node
                    // Reconstruct moves from current handle
                    let mut moves: Vec<(usize, &'static str)> = Vec::new();
                    let mut walk = cur_handle;
                    while let Some(parent_handle) = walk.parent(&mut pool) {
                        let idx = walk.component_index(&pool).unwrap_or(0);
                        let action = walk.transition_info(&pool).map(|t| t.action).unwrap_or("unknown");
                        moves.push((idx, action));
                        walk = parent_handle;
                    }
                    moves.reverse();
                    return (moves, cur.clone(), cur_cost + heuristic_impl.lower_bound(cur, &desc, target_type, target));
                }
            }
        }
        if opts.prune {
            let ub_suffix = heuristic_impl.upper_bound(cur, &desc, target_type, target);
            let candidate_total = cur_cost + ub_suffix;
            if candidate_total <= best_ub { best_ub = candidate_total; } else { continue; }
        }
        let successors = super::iter_successors(cur, &desc).collect::<Vec<_>>();
        for successor in successors {
            let cost_value = cur_cost + successor.step_cost;
            let h = heuristic_impl.lower_bound(&successor.next_state, &desc, target_type, target);
            if opts.prune {
                let ub_ns = heuristic_impl.upper_bound(&successor.next_state, &desc, target_type, target);
                if cost_value + ub_ns > best_ub { pruned += 1; continue; } else { best_ub = cost_value + ub_ns; }
            }
            pool.enqueue_or_update_state(
                successor.next_state,
                cost_value,
                Some(&cur_handle),
                successor.node_index,
                Some(super::TransitionInfo { action: successor.action, cost: successor.step_cost }),
                cost_value + h,
            );
        }
    }

    if let Some(goal_handle) = goal_i {
        let final_state: State = goal_handle.state(&pool).unwrap().clone();
        let mut moves: Vec<(usize, &'static str)> = Vec::new();
        let mut walk = goal_handle;
        while let Some(parent_handle) = walk.parent(&mut pool) {
            let idx = walk.component_index(&pool).unwrap_or(0);
            let action = walk.transition_info(&pool).map(|t| t.action).unwrap_or("unknown");
            moves.push((idx, action));
            walk = parent_handle;
        }
        moves.reverse();
        (moves, final_state, goal_g)
    } else {
        // Exhausted; return empty plan and start state with 0 cost
        (Vec::new(), start, 0.0)
    }
}


