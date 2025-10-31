use crate::heuristic::create_by_name;
use crate::state_pool::{StateHandle, StatePool};
use crate::{NodeDesc, State, TargetType, is_terminal};

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

#[allow(private_interfaces)]
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
    // Smaller initial heap bound to help surface growth-related issues sooner during debugging
    let mut pool = StatePool::<State, super::TransitionInfo>::new(100_000);
    let h0 = heuristic_impl.lower_bound(&start, &desc, target_type, target);
    pool.enqueue_or_update_state(start.clone(), 0.0, None, 0, None, h0);
    let mut best_ub = heuristic_impl.upper_bound(&start, &desc, target_type, target);
    #[cfg(debug_assertions)]
    let mut prev_best_ub: Option<f64> = None;
    let mut expanded: usize = 0;
    let mut goal_i: Option<StateHandle<State, super::TransitionInfo>> = None;
    let mut goal_g: f64 = 0.0;
    let mut pruned: usize = 0;

    while let Some(cur_handle) = pool.heap_pop() {
        expanded += 1;

        let cur_cost = cur_handle.cost_from_start(&pool);
        let cur_state = cur_handle.state(&pool).unwrap().clone();
        if is_terminal(&cur_state, target_type, target) {
            goal_g = cur_cost;
            goal_i = Some(cur_handle);
            break;
        }
        if let Some(cb) = opts.progress_cb.as_mut()
            && opts.print_every > 0
            && (expanded == 1 || expanded.is_multiple_of(opts.print_every))
        {
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
                    let action = walk
                        .transition_info(&pool)
                        .map(|t| t.action)
                        .unwrap_or("unknown");
                    moves.push((idx, action));
                    walk = parent_handle;
                }
                moves.reverse();
                return (
                    moves,
                    cur_state.clone(),
                    cur_cost + heuristic_impl.lower_bound(&cur_state, &desc, target_type, target),
                );
            }
        }
        if opts.prune {
            let ub_suffix = heuristic_impl.upper_bound(&cur_state, &desc, target_type, target);
            let candidate_total = cur_cost + ub_suffix;
            if candidate_total <= best_ub {
                let old_best_ub = best_ub;
                best_ub = candidate_total;
                #[cfg(debug_assertions)]
                {
                    if let Some(prev) = prev_best_ub {
                        debug_assert!(
                            best_ub <= prev,
                            "best_ub must never increase (was {}, now {})",
                            prev,
                            best_ub
                        );
                    }
                    debug_assert!(
                        best_ub <= old_best_ub,
                        "best_ub must never increase (was {}, now {})",
                        old_best_ub,
                        best_ub
                    );
                    prev_best_ub = Some(best_ub);
                }
            } else {
                continue;
            }
        }
        let successors = super::iter_successors(&cur_state, &desc).collect::<Vec<_>>();
        for successor in successors {
            let cost_value = cur_cost + successor.step_cost;
            let h = heuristic_impl.lower_bound(&successor.next_state, &desc, target_type, target);
            if opts.prune {
                let ub_ns =
                    heuristic_impl.upper_bound(&successor.next_state, &desc, target_type, target);
                if cost_value + ub_ns > best_ub {
                    pruned += 1;
                    continue;
                } else {
                    let old_best_ub = best_ub;
                    best_ub = cost_value + ub_ns;
                    #[cfg(debug_assertions)]
                    {
                        if let Some(prev) = prev_best_ub {
                            debug_assert!(
                                best_ub <= prev,
                                "best_ub must never increase (was {}, now {})",
                                prev,
                                best_ub
                            );
                        }
                        debug_assert!(
                            best_ub <= old_best_ub,
                            "best_ub must never increase (was {}, now {})",
                            old_best_ub,
                            best_ub
                        );
                        prev_best_ub = Some(best_ub);
                    }
                }
            }
            pool.enqueue_or_update_state(
                successor.next_state,
                cost_value,
                Some(&cur_handle),
                successor.node_index,
                Some(super::TransitionInfo {
                    action: successor.action,
                    cost: successor.step_cost,
                }),
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
            let action = walk
                .transition_info(&pool)
                .map(|t| t.action)
                .unwrap_or("unknown");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_desc() -> Vec<NodeDesc> {
        // Two nodes with small slots
        vec![NodeDesc { slots: 3 }, NodeDesc { slots: 2 }]
    }

    fn make_start() -> State {
        // infra=0, civ=1, mil=0 on first; infra=0, civ=0, mil=0 on second
        State(vec![
            crate::NodeState {
                infra: 0,
                civ: 1,
                mil: 0,
            },
            crate::NodeState {
                infra: 0,
                civ: 0,
                mil: 0,
            },
        ])
    }

    #[test]
    fn prune_does_not_expand_more_than_no_prune_and_cost_matches() {
        let desc = make_desc();
        let start = make_start();
        // Track iterations via callback
        let mut iters_prune = 0usize;
        let mut iters_noprune = 0usize;
        let mut cb_prune = |snap: &ProgressSnapshot| {
            iters_prune = snap.iterations;
            false
        };
        let mut cb_noprune = |snap: &ProgressSnapshot| {
            iters_noprune = snap.iterations;
            false
        };
        let opts_prune = SolveOptions {
            prune: true,
            print_every: 1,
            heuristic_name: "best_infra_upper_bound",
            progress_cb: Some(&mut cb_prune),
        };
        let opts_noprune = SolveOptions {
            prune: false,
            print_every: 1,
            heuristic_name: "best_infra_upper_bound",
            progress_cb: Some(&mut cb_noprune),
        };
        let (_m1, _s1, cost_prune) = solve_and_reconstruct_core(
            desc.clone(),
            start.clone(),
            crate::TargetType::Military,
            2,
            opts_prune,
        );
        let (_m2, _s2, cost_noprune) = solve_and_reconstruct_core(
            desc.clone(),
            start.clone(),
            crate::TargetType::Military,
            2,
            opts_noprune,
        );
        assert!((cost_prune - cost_noprune).abs() < 1e-9, "costs must match");
        assert!(
            iters_prune <= iters_noprune,
            "prune should not expand more nodes"
        );
    }

    #[test]
    fn test_heuristic_bounds_invariants() {
        // Test that bounds satisfy basic invariants: non-negativity and ordering
        use crate::heuristic::create_by_name;
        let h = create_by_name("best_infra_upper_bound").unwrap();
        let desc = make_desc();
        let start = make_start();

        // Test non-negativity
        let lb = h.lower_bound(&start, &desc, crate::TargetType::Military, 2);
        let ub = h.upper_bound(&start, &desc, crate::TargetType::Military, 2);
        assert!(lb >= 0.0, "lower_bound must be non-negative, got {}", lb);
        assert!(
            ub >= 0.0 || ub == f64::INFINITY,
            "upper_bound must be non-negative or infinity, got {}",
            ub
        );

        // Test ordering: lower_bound <= upper_bound
        if ub != f64::INFINITY {
            assert!(
                lb <= ub + 1e-9,
                "lower_bound must be <= upper_bound, got lb={}, ub={}",
                lb,
                ub
            );
        }

        // Test on different target types
        for &target_type in &[
            crate::TargetType::Military,
            crate::TargetType::Civilian,
            crate::TargetType::Factories,
        ] {
            let lb = h.lower_bound(&start, &desc, target_type, 2);
            let ub = h.upper_bound(&start, &desc, target_type, 2);
            assert!(
                lb >= 0.0,
                "lower_bound must be non-negative for {:?}, got {}",
                target_type,
                lb
            );
            assert!(
                ub >= 0.0 || ub == f64::INFINITY,
                "upper_bound must be non-negative or infinity for {:?}, got {}",
                target_type,
                ub
            );
            if ub != f64::INFINITY {
                assert!(
                    lb <= ub + 1e-9,
                    "lower_bound must be <= upper_bound for {:?}, got lb={}, ub={}",
                    target_type,
                    lb,
                    ub
                );
            }
        }
    }

    #[test]
    fn heuristic_is_consistent_on_sample_successors() {
        let desc = make_desc();
        let start = make_start();
        let h = create_by_name("best_infra_upper_bound").unwrap();
        let h_s = h.lower_bound(&start, &desc, crate::TargetType::Factories, 3);
        for succ in crate::iter_successors(&start, &desc) {
            let h_sp = h.lower_bound(&succ.next_state, &desc, crate::TargetType::Factories, 3);
            assert!(
                h_s <= succ.step_cost + h_sp + 1e-9,
                "consistency violated: h(s)={} > c+ h(s')={} + {}",
                h_s,
                succ.step_cost,
                h_sp
            );
        }
    }

    // Property-based tests using proptest
    use proptest::prelude::*;
    use proptest::strategy::Union;

    fn arb_desc() -> impl Strategy<Value = Vec<NodeDesc>> {
        prop::collection::vec(1u8..=5u8, 1..=4)
            .prop_map(|slots| slots.into_iter().map(|s| NodeDesc { slots: s }).collect())
    }
    fn arb_state_from(desc: Vec<NodeDesc>) -> impl Strategy<Value = State> {
        let n = desc.len();
        let per = desc.into_iter().map(|node_desc| {
            (0u8..=5u8, 0u8..=node_desc.slots, 0u8..=node_desc.slots)
                .prop_filter("capacity", move |(_i, c, m)| {
                    c.saturating_add(*m) <= node_desc.slots
                })
        });
        prop::collection::vec(Union::new(per.collect::<Vec<_>>()), n..=n).prop_map(|v| {
            State(
                v.into_iter()
                    .map(|(infra, civ, mil)| crate::NodeState { infra, civ, mil })
                    .collect(),
            )
        })
    }

    proptest! {
        #[test]
        fn prop_lb_le_ub((desc, st) in arb_desc().prop_flat_map(|d| (Just(d.clone()), arb_state_from(d)))) {
            let h = create_by_name("best_infra_upper_bound").unwrap();
            for &tt in &[crate::TargetType::Military, crate::TargetType::Civilian, crate::TargetType::Factories] {
                let lb = h.lower_bound(&st, &desc, tt, 0);
                let ub = h.upper_bound(&st, &desc, tt, 0);
                prop_assert!(lb <= ub + 1e-9 && lb >= 0.0 && ub >= 0.0);
            }
        }

        #[test]
        fn prop_consistency((desc, st) in arb_desc().prop_flat_map(|d| (Just(d.clone()), arb_state_from(d)))) {
            let h = create_by_name("best_infra_upper_bound").unwrap();
            for &tt in &[crate::TargetType::Military, crate::TargetType::Civilian, crate::TargetType::Factories] {
                let hs = h.lower_bound(&st, &desc, tt, 1);
                for succ in crate::iter_successors(&st, &desc) {
                    let hsp = h.lower_bound(&succ.next_state, &desc, tt, 1);
                    prop_assert!(hs <= succ.step_cost + hsp + 1e-9);
                }
            }
        }
    }
}
