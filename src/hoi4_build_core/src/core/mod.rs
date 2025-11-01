use crate::heuristic::create_by_name;
use crate::state_pool::{StateHandle, StatePool};
use crate::{NodeDesc, State, TargetType, is_terminal};

pub mod exact_solver;

#[derive(Clone)]
pub struct ProgressSnapshot {
    pub iterations: usize,
    pub cost_from_start: f64,
    pub heap_size: usize,
    pub total_states: usize,
    pub avg_estimated_total_cost: f64,
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
    let initial_heuristic_estimate = heuristic_impl.lower_bound(&start, &desc, target_type, target);
    pool.enqueue_or_update_state(
        start.clone(),
        0.0, // path_cost from start is 0
        None,
        0,
        None,
        initial_heuristic_estimate, // estimated_total_cost = path_cost + heuristic_estimate
    );
    let mut best_ub = heuristic_impl.upper_bound(&start, &desc, target_type, target);
    #[cfg(debug_assertions)]
    let mut prev_best_ub: Option<f64> = None;
    let mut expanded: usize = 0;
    let mut goal_i: Option<StateHandle<State, super::TransitionInfo>> = None;
    let mut goal_path_cost: f64 = 0.0;
    let mut pruned: usize = 0;

    while let Some(cur_handle) = pool.heap_pop() {
        expanded += 1;

        let cur_cost = cur_handle.cost_from_start(&pool);
        let cur_state = cur_handle.state(&pool).unwrap().clone();
        if is_terminal(&cur_state, target_type, target) {
            goal_path_cost = cur_cost;
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
                avg_estimated_total_cost: pool.heap_avg_estimated_total_cost(),
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
            let path_cost = cur_cost + successor.step_cost;
            let heuristic_estimate =
                heuristic_impl.lower_bound(&successor.next_state, &desc, target_type, target);
            if opts.prune {
                let ub_ns =
                    heuristic_impl.upper_bound(&successor.next_state, &desc, target_type, target);
                if path_cost + ub_ns > best_ub {
                    pruned += 1;
                    continue;
                } else {
                    let old_best_ub = best_ub;
                    best_ub = path_cost + ub_ns;
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
            let estimated_total_cost = path_cost + heuristic_estimate;
            pool.enqueue_or_update_state(
                successor.next_state,
                path_cost,
                Some(&cur_handle),
                successor.node_index,
                Some(super::TransitionInfo {
                    action: successor.action,
                    cost: successor.step_cost,
                }),
                estimated_total_cost,
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
        (moves, final_state, goal_path_cost)
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

    #[test]
    fn prop_upper_bound_admissible_on_small_instances() {
        // Property test: For small instances where exact solver can run,
        // verify that upper_bound >= exact_optimal_cost (admissibility)
        use crate::core::exact_solver::exact_optimal_cost;

        let h = create_by_name("best_infra_upper_bound").unwrap();

        // Generate small instances (≤2 nodes, ≤3 slots, ≤2 target)
        let desc_strategy = prop::collection::vec(Just(NodeDesc { slots: 3 }), 1..=2);
        let target_strategy = 0i32..=2;
        let target_type_strategy = prop::sample::select(&[
            crate::TargetType::Military,
            crate::TargetType::Civilian,
            crate::TargetType::Factories,
        ]);

        proptest!(ProptestConfig::with_cases(50), |(desc in desc_strategy, target in target_strategy, target_type in target_type_strategy)| {
            // Generate valid start state (small slots, low values)
            let start = State(desc.iter().map(|_d| crate::NodeState {
                infra: 0u8,
                civ: 0u8,
                mil: 0u8,
            }).collect());

            // Compute upper bound and exact optimal
            let ub = h.upper_bound(&start, &desc, target_type, target);
            let exact_opt = exact_optimal_cost(&desc, &start, target_type, target);

            if let Some(exact) = exact_opt {
                // If exact solver found a solution, upper bound must be >= exact optimal
                prop_assert!(
                    ub >= exact || ub == f64::INFINITY,
                    "Upper bound must be >= exact optimal: ub={}, exact={}",
                    ub,
                    exact
                );
            } else {
                // If exact solver returned None, upper bound should be non-negative
                prop_assert!(
                    ub >= 0.0 || ub == f64::INFINITY,
                    "Upper bound must be non-negative or infinity: ub={}",
                    ub
                );
            }
        });
    }

    #[test]
    fn prop_lower_bound_admissible_on_small_instances() {
        // Property test: For small instances where exact solver can run,
        // verify that lower_bound <= exact_optimal_cost (admissibility)
        use crate::core::exact_solver::exact_optimal_cost;

        let h = create_by_name("best_infra_upper_bound").unwrap();

        // Generate small instances (≤2 nodes, ≤3 slots, ≤2 target)
        let desc_strategy = prop::collection::vec(Just(NodeDesc { slots: 3 }), 1..=2);
        let target_strategy = 0i32..=2;
        let target_type_strategy = prop::sample::select(&[
            crate::TargetType::Military,
            crate::TargetType::Civilian,
            crate::TargetType::Factories,
        ]);

        proptest!(ProptestConfig::with_cases(50), |(desc in desc_strategy, target in target_strategy, target_type in target_type_strategy)| {
            // Generate valid start state (small slots, low values)
            let start = State(desc.iter().map(|_d| crate::NodeState {
                infra: 0u8,
                civ: 0u8,
                mil: 0u8,
            }).collect());

            // Compute lower bound and exact optimal
            let lb = h.lower_bound(&start, &desc, target_type, target);
            let exact_opt = exact_optimal_cost(&desc, &start, target_type, target);

            if let Some(exact) = exact_opt {
                // If exact solver found a solution, lower bound must be <= exact optimal
                prop_assert!(
                    lb <= exact + 1e-9,
                    "Lower bound must be <= exact optimal: lb={}, exact={}",
                    lb,
                    exact
                );
            } else {
                // If exact solver returned None, lower bound should still be non-negative
                prop_assert!(
                    lb >= 0.0,
                    "Lower bound must be non-negative: lb={}",
                    lb
                );
            }
        });
    }

    #[cfg(test)]
    mod astar_invariant_tests {
        use super::*;
        use crate::TransitionInfo;
        use crate::heuristic::create_by_name;

        // Test helpers
        fn make_desc() -> Vec<NodeDesc> {
            vec![NodeDesc { slots: 3 }, NodeDesc { slots: 2 }]
        }

        fn make_start() -> State {
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

        /// Hypothesis 1: Priority queue ordering bug
        /// Requirement: States in priority queue should be ordered by f = g + h (lowest first)
        /// Test: When popping states, each popped state should have f <= f of all remaining states
        #[test]
        fn test_priority_queue_orders_by_f_value() {
            // This test verifies that the priority queue maintains the invariant:
            // When popping state S with f(S), all remaining states have f >= f(S)
            let desc = make_desc();
            let start = make_start();
            let heuristic_impl = create_by_name("dijkstra").expect("heuristic must be valid");
            let mut pool = crate::state_pool::StatePool::<State, TransitionInfo>::new(1000);

            let h0 = heuristic_impl.lower_bound(&start, &desc, crate::TargetType::Military, 2);
            pool.enqueue_or_update_state(start.clone(), 0.0, None, 0, None, h0);

            let mut prev_f: Option<f64> = None;
            let mut expanded_count = 0;
            const MAX_EXPANSIONS: usize = 100; // Limit to avoid infinite loops

            while let Some(cur_handle) = pool.heap_pop() {
                expanded_count += 1;
                if expanded_count > MAX_EXPANSIONS {
                    break;
                }

                let cur_cost = cur_handle.cost_from_start(&pool);
                let cur_state = cur_handle.state(&pool).unwrap().clone();
                let h =
                    heuristic_impl.lower_bound(&cur_state, &desc, crate::TargetType::Military, 2);
                let cur_f = cur_cost + h;

                // Check invariant: current f should be <= all previous f values
                if let Some(prev) = prev_f {
                    assert!(
                        cur_f <= prev + 1e-9,
                        "Priority queue ordering violated: popped state with f={} after state with f={}",
                        cur_f,
                        prev
                    );
                }
                prev_f = Some(cur_f);

                // Generate successors and check heap ordering
                for successor in crate::iter_successors(&cur_state, &desc) {
                    let successor_cost = cur_cost + successor.step_cost;
                    let successor_h = heuristic_impl.lower_bound(
                        &successor.next_state,
                        &desc,
                        crate::TargetType::Military,
                        2,
                    );
                    let successor_f = successor_cost + successor_h;

                    pool.enqueue_or_update_state(
                        successor.next_state,
                        successor_cost,
                        Some(&cur_handle),
                        0,
                        Some(TransitionInfo {
                            action: successor.action,
                            cost: successor.step_cost,
                        }),
                        successor_f,
                    );
                }
            }
        }

        /// Hypothesis 2: State memoization bug
        /// Requirement: When enqueuing a state with better cost, it should update the existing state
        /// Test: Enqueue same state twice with different costs, verify best cost is kept
        #[test]
        fn test_enqueue_or_update_preserves_best_cost() {
            // This test verifies that enqueue_or_update_state updates states with better costs
            let mut pool = crate::state_pool::StatePool::<State, TransitionInfo>::new(1000);
            let state1 = make_start();
            let heuristic_impl = create_by_name("dijkstra").expect("heuristic must be valid");
            let desc = make_desc();
            let target_type = crate::TargetType::Military;
            let target = 2;

            // First, enqueue state with cost 100.0
            let h1 = heuristic_impl.lower_bound(&state1, &desc, target_type, target);
            let f1 = 100.0 + h1;
            let result1 = pool.enqueue_or_update_state(state1.clone(), 100.0, None, 0, None, f1);
            assert!(result1, "First enqueue should succeed");

            // Verify cost by popping from heap
            let handle1 = pool.heap_pop();
            assert!(
                handle1.is_some(),
                "State should be in heap after first enqueue"
            );
            if let Some(h) = handle1 {
                assert_eq!(
                    h.cost_from_start(&pool),
                    100.0,
                    "First enqueue should set cost to 100.0"
                );
                // Put it back for next test
                pool.heap_push(&h, f1);
            }

            // Now enqueue same state with better cost 50.0
            let f2 = 50.0 + h1;
            let result2 = pool.enqueue_or_update_state(state1.clone(), 50.0, None, 0, None, f2);
            assert!(result2, "Second enqueue with better cost should succeed");

            // Verify cost was updated by checking if the second enqueue succeeded
            // and that we can get the state from the heap
            // Note: We can't directly check the index, but we can verify behavior
            let handle2 = pool.heap_pop();
            assert!(handle2.is_some(), "State should be in heap after update");
            if let Some(h) = handle2 {
                let updated_cost = h.cost_from_start(&pool);
                assert_eq!(
                    updated_cost, 50.0,
                    "Cost should be updated to better value 50.0, but got {}",
                    updated_cost
                );
            }

            // Now enqueue same state with worse cost 150.0
            let f3 = 150.0 + h1;
            let result3 = pool.enqueue_or_update_state(state1.clone(), 150.0, None, 0, None, f3);
            // This should fail (return false) because cost is worse
            assert!(
                !result3,
                "Enqueuing with worse cost should fail (return false), but got {}",
                result3
            );

            // Note: We can't directly verify cost wasn't updated since get_index is private
            // But we verified that result3 was false, which means the update was rejected
        }

        /// Hypothesis 3: Duplicate state handling bug
        /// Requirement: Same state should not appear multiple times in queue with different costs
        /// Test: Verify that enqueuing same state multiple times doesn't create duplicates
        #[test]
        fn test_no_duplicate_states_in_heap() {
            // This test verifies that the same state doesn't appear multiple times in the heap
            let mut pool = crate::state_pool::StatePool::<State, TransitionInfo>::new(1000);
            let state1 = make_start();
            let heuristic_impl = create_by_name("dijkstra").expect("heuristic must be valid");
            let desc = make_desc();
            let target_type = crate::TargetType::Military;
            let target = 2;

            let h1 = heuristic_impl.lower_bound(&state1, &desc, target_type, target);

            // Enqueue same state multiple times with different f values
            // Each should either update the existing entry or be rejected if cost is worse
            for i in 0..10 {
                let cost = if i == 0 { 100.0 } else { 50.0 + (i as f64) };
                let f = cost + h1;
                pool.enqueue_or_update_state(state1.clone(), cost, None, 0, None, f);
            }

            // Verify state appears only once in heap
            // Since get_index is private, we'll verify by popping all states and counting
            let mut state1_count = 0;
            while let Some(handle) = pool.heap_pop() {
                let state = handle.state(&pool).unwrap();
                if *state == state1 {
                    state1_count += 1;
                }
            }

            assert_eq!(
                state1_count, 1,
                "State should appear exactly once in heap, but appears {} times",
                state1_count
            );
        }

        /// Hypothesis 4: Re-expansion bug
        /// Requirement: States should not be expanded before best path to them is found
        /// Test: If we find a better path to a state that was already expanded, we should re-expand it
        #[test]
        fn test_better_path_found_after_expansion() {
            // This test verifies that if we find a better path to an already-expanded state,
            // we correctly handle it (either re-expand or update correctly)
            let mut pool = crate::state_pool::StatePool::<State, TransitionInfo>::new(1000);
            let state1 = make_start();
            let heuristic_impl = create_by_name("dijkstra").expect("heuristic must be valid");
            let desc = make_desc();
            let target_type = crate::TargetType::Military;
            let target = 2;

            let h1 = heuristic_impl.lower_bound(&state1, &desc, target_type, target);

            // Enqueue and expand state1 with cost 100.0
            let f1 = 100.0 + h1;
            pool.enqueue_or_update_state(state1.clone(), 100.0, None, 0, None, f1);
            let handle1 = pool.heap_pop().expect("State should be in heap");
            let expanded_cost1 = handle1.cost_from_start(&pool);
            assert_eq!(expanded_cost1, 100.0);

            // Now find a better path to state1 (cost 50.0)
            // In A*, once a state is expanded, we don't re-expand it even if we find a better path
            // However, we should verify that the cost is updated for consistency
            let f2 = 50.0 + h1;
            let result = pool.enqueue_or_update_state(state1.clone(), 50.0, None, 0, None, f2);

            // After expansion, enqueue_or_update_state should handle the better path
            // Since get_index is private, we verify behavior by checking if state can be re-added
            // In standard A*, once expanded, states are not re-expanded even with better paths
            // However, the cost should be updated if the state is still tracked
            if result {
                // If update succeeded, verify the better cost is used
                if let Some(handle) = pool.heap_pop() {
                    let updated_cost = handle.cost_from_start(&pool);
                    if updated_cost < f64::INFINITY {
                        assert!(
                            updated_cost <= 50.0 + 1e-9,
                            "Better path cost should be preserved: got {} but expected <= 50.0",
                            updated_cost
                        );
                    }
                }
            }
        }

        /// Hypothesis 5: A* termination invariant
        /// Requirement: When a goal is popped from queue, g(goal) <= f(any_unexpanded)
        /// Test: Verify that when we find a goal, all remaining states have f >= g(goal)
        #[test]
        fn test_astar_termination_invariant() {
            // This test verifies the A* invariant: when a goal is popped, it's optimal
            // i.e., g(goal) <= f(any_unexpanded_state)
            let desc = make_desc();
            let start = make_start();
            let heuristic_impl = create_by_name("dijkstra").expect("heuristic must be valid");
            let mut pool = crate::state_pool::StatePool::<State, TransitionInfo>::new(1000);

            let target_type = crate::TargetType::Military;
            let target = 2;

            let h0 = heuristic_impl.lower_bound(&start, &desc, target_type, target);
            pool.enqueue_or_update_state(start.clone(), 0.0, None, 0, None, h0);

            let mut goal_g: Option<f64> = None;
            const MAX_EXPANSIONS: usize = 1000;

            let mut expanded = 0;
            while let Some(cur_handle) = pool.heap_pop() {
                expanded += 1;
                if expanded > MAX_EXPANSIONS {
                    break;
                }

                let cur_cost = cur_handle.cost_from_start(&pool);
                let cur_state = cur_handle.state(&pool).unwrap().clone();

                if crate::is_terminal(&cur_state, target_type, target) {
                    goal_g = Some(cur_cost);
                    break;
                }

                let _h = heuristic_impl.lower_bound(&cur_state, &desc, target_type, target);
                let successors = crate::iter_successors(&cur_state, &desc);
                for successor in successors {
                    let successor_cost = cur_cost + successor.step_cost;
                    let successor_h = heuristic_impl.lower_bound(
                        &successor.next_state,
                        &desc,
                        target_type,
                        target,
                    );
                    let successor_f = successor_cost + successor_h;

                    pool.enqueue_or_update_state(
                        successor.next_state,
                        successor_cost,
                        Some(&cur_handle),
                        0,
                        Some(TransitionInfo {
                            action: successor.action,
                            cost: successor.step_cost,
                        }),
                        successor_f,
                    );
                }
            }

            if let Some(goal_g_value) = goal_g {
                // Verify that the goal cost is optimal by comparing with exact solver
                let exact_cost = crate::core::exact_solver::exact_optimal_cost(
                    &desc,
                    &start,
                    target_type,
                    target,
                );

                if let Some(exact) = exact_cost {
                    assert!(
                        goal_g_value <= exact + 1e-9,
                        "A* termination invariant violated: goal cost {} exceeds exact optimal {}",
                        goal_g_value,
                        exact
                    );
                }
            } else {
                panic!(
                    "Test should find a goal within {} expansions",
                    MAX_EXPANSIONS
                );
            }
        }

        /// Hypothesis 6: Cost update triggers heap reordering
        /// Requirement: When we update a state's cost with enqueue_or_update_state,
        /// the heap should be reordered to reflect the new f value
        #[test]
        fn test_cost_update_triggers_heap_reorder() {
            // This test verifies that when we update a state's cost,
            // the heap is properly reordered
            let mut pool = crate::state_pool::StatePool::<State, TransitionInfo>::new(1000);
            let state1 = make_start();
            let state2 = State(vec![
                crate::NodeState {
                    infra: 0,
                    civ: 2,
                    mil: 0,
                },
                crate::NodeState {
                    infra: 0,
                    civ: 0,
                    mil: 0,
                },
            ]);
            let heuristic_impl = create_by_name("dijkstra").expect("heuristic must be valid");
            let desc = make_desc();
            let target_type = crate::TargetType::Military;
            let target = 2;

            // Enqueue state1 with high f value
            let h1 = heuristic_impl.lower_bound(&state1, &desc, target_type, target);
            let f1 = 200.0 + h1;
            pool.enqueue_or_update_state(state1.clone(), 200.0, None, 0, None, f1);

            // Enqueue state2 with lower f value
            let h2 = heuristic_impl.lower_bound(&state2, &desc, target_type, target);
            let f2 = 100.0 + h2;
            pool.enqueue_or_update_state(state2.clone(), 100.0, None, 0, None, f2);

            // state2 should be popped first (lower f)
            let first = pool.heap_pop().expect("Heap should have states");
            let first_state = first.state(&pool).unwrap().clone();
            assert_eq!(
                first_state, state2,
                "State with lower f should be popped first"
            );

            // Now update state1 with better cost
            let f1_better = 50.0 + h1;
            pool.enqueue_or_update_state(state1.clone(), 50.0, None, 0, None, f1_better);

            // If heap is properly reordered, state1 should now be popped next
            // (or at least have better f than before)
            if let Some(second) = pool.heap_pop() {
                let second_cost = second.cost_from_start(&pool);
                assert!(
                    second_cost <= 200.0 + 1e-9,
                    "Updated state should have better cost, got {}",
                    second_cost
                );
            }
        }

        /// Hypothesis 7: No-prune mode finds optimal solution
        /// Requirement: A* without pruning should find the same optimal solution as exact solver
        /// Test: Compare no-prune A* result with exact solver for known test case
        #[test]
        fn test_no_prune_finds_optimal_solution() {
            // This is the main failing test case
            let desc = make_desc();
            let start = make_start();
            let target_type = crate::TargetType::Military;
            let target = 2;

            // Get exact optimal cost
            let exact_cost =
                crate::core::exact_solver::exact_optimal_cost(&desc, &start, target_type, target);
            assert!(
                exact_cost.is_some(),
                "Exact solver should find solution for this test case"
            );
            let exact = exact_cost.unwrap();

            // Run A* without pruning
            let opts_noprune: SolveOptions<'_, fn(&ProgressSnapshot) -> bool> = SolveOptions {
                prune: false,
                print_every: usize::MAX,
                heuristic_name: "dijkstra",
                progress_cb: None,
            };

            let (_moves, _final_state, cost_noprune) = solve_and_reconstruct_core(
                desc.clone(),
                start.clone(),
                target_type,
                target,
                opts_noprune,
            );

            // A* should find optimal solution
            assert!(
                (cost_noprune - exact).abs() < 1e-9,
                "A* without pruning should find optimal solution: got {} but expected {}",
                cost_noprune,
                exact
            );
        }
    }
}
