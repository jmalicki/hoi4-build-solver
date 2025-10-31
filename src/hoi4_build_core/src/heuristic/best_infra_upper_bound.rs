//! Best-case infrastructure + upper-bound civilians heuristic.
//!
//! This heuristic assumes:
//! - Best-case infra multiplier (as if infra=5 on the acting node), and
//! - civUpper = current civilians + max(0, emptySlots - remaining) as an
//!   upper bound for the denominator effect.
//!
//! See the module README.md for detailed theoretical background and proof of admissibility/consistency.

use super::Heuristic;
use crate::{NodeDesc, State, TargetType};
use std::cmp::Ordering;

/// Compute infrastructure multiplier for a given level in [0,5].
fn infra_mult(infra: u8) -> f64 {
    1.0 + (2.0 * (infra as f64)) / 10.0
}

/// Best-case infrastructure with upper-bound civilians heuristic.
///
/// Provides both an admissible lower bound and an upper bound based on
/// a "convert then build military" greedy strategy.
#[derive(Clone, Copy, Debug)]
pub struct BestInfraUpperBoundHeuristic;

impl Heuristic for BestInfraUpperBoundHeuristic {
    /// Admissible lower bound: optimistic estimate using best-case infra and conversion awareness.
    fn lower_bound(
        &self,
        st: &State,
        nodes: &[NodeDesc],
        target_type: TargetType,
        target: i32,
    ) -> f64 {
        let mut cur_mil = 0i32;
        let mut cur_civ = 0i32;
        let mut sum_civ = 0i32;
        let mut empty = 0i32;
        for (ns, node_desc) in st.0.iter().zip(nodes.iter()) {
            cur_mil += ns.mil as i32;
            cur_civ += ns.civ as i32;
            sum_civ += ns.civ as i32;
            let used = ns.civ as i32 + ns.mil as i32;
            empty += ((node_desc.slots as i32) - used).max(0);
        }

        let remaining = match target_type {
            TargetType::Military => (target - cur_mil).max(0),
            TargetType::Civilian => (target - cur_civ).max(0),
            TargetType::Factories => (target - (cur_mil + cur_civ)).max(0),
        };

        if remaining == 0 {
            return 0.0;
        }

        // Optimistic denominator bound (global), as in docs/MODELING.md
        let civ_upper = (sum_civ + (empty - remaining).max(0)).max(1) as f64;
        // Using max infra (5) is valid for a lower bound: the infra multiplier appears in the
        // denominator, so using the maximum multiplier gives the minimum cost estimate, ensuring
        // the bound remains admissible (≤ actual cost).
        let best_mult = 1.0 + (2.0 * 5.0) / 10.0;

        match target_type {
            TargetType::Military => {
                // Tighter base-cost blend: at most current civilians can be converted at 4000 base.
                // The rest must be built as military at 7200 base.
                let conv_usable = remaining.min(sum_civ);
                let mil_needed = remaining - conv_usable;
                let blended_base = (4000.0 * (conv_usable as f64)) + (7200.0 * (mil_needed as f64));
                blended_base / best_mult / civ_upper
            }
            TargetType::Civilian => {
                // For civilian factories, we can only build (not convert).
                // Base cost is 10800 for civilian factories.
                let blended_base = 10800.0 * (remaining as f64);
                blended_base / best_mult / civ_upper
            }
            TargetType::Factories => {
                // For total factories, conversions don't change the count, so we must build new factories.
                // Build the cheapest type (military is cheaper than civilian).
                let blended_base = 7200.0 * (remaining as f64); // Use military cost (cheapest)
                blended_base / best_mult / civ_upper
            }
        }
    }

    /// Upper bound: finish by first converting civilians to military (if applicable), then building.
    ///
    /// Strategy depends on target type:
    /// - Military: convert civilians to military, then build military
    /// - Civilian: build civilian factories only
    /// - Factories: convert then build (any mix counts toward total)
    ///
    /// - Conversion stage (for military/factories): up to min(remaining, total_civ), choose cheapest nodes by 4000/mult.
    ///   The global denominator decreases by 1 per conversion.
    /// - Build stage: allocate remaining to cheapest nodes by build cost/mult, limited by empties,
    ///   using the (post-conversion) civilian denominator which is constant during builds.
    fn upper_bound(
        &self,
        st: &State,
        nodes: &[NodeDesc],
        target_type: TargetType,
        target: i32,
    ) -> f64 {
        let mut cur_mil = 0i32;
        let mut cur_civ = 0i32;
        let mut total_civ = 0i32;
        let mut empties_per_node: Vec<(f64, f64, f64, i32, i32)> = Vec::with_capacity(st.0.len());
        // store (conv_unit_num, build_mil_num, build_civ_num, civ_count, empty_slots)
        for (ns, node_desc) in st.0.iter().zip(nodes.iter()) {
            cur_mil += ns.mil as i32;
            cur_civ += ns.civ as i32;
            total_civ += ns.civ as i32;
            let used = ns.civ as i32 + ns.mil as i32;
            let empty = ((node_desc.slots as i32) - used).max(0);
            let mult = infra_mult(ns.infra);
            let conv_num = 4000.0 / mult; // denominator applied later
            let build_mil_num = 7200.0 / mult;
            let build_civ_num = 10800.0 / mult;
            empties_per_node.push((conv_num, build_mil_num, build_civ_num, ns.civ as i32, empty));
        }

        let mut need = match target_type {
            TargetType::Military => (target - cur_mil).max(0),
            TargetType::Civilian => (target - cur_civ).max(0),
            TargetType::Factories => (target - (cur_mil + cur_civ)).max(0),
        };

        if need == 0 {
            return 0.0;
        }

        match target_type {
            TargetType::Military => {
                // Convert civilians to military, then build military
                let mut ub = 0.0f64;
                let conv_cap = need.min(total_civ);
                let mut empties = empties_per_node.clone();

                // Conversion stage
                if conv_cap > 0 {
                    empties.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
                    let mut conv_done = 0i32;
                    let mut civ_den = total_civ.max(1) as f64;
                    let mut i = 0usize;
                    while conv_done < conv_cap {
                        while i < empties.len() && empties[i].3 <= 0 {
                            i += 1;
                        }
                        if i >= empties.len() || empties[i].3 <= 0 {
                            break;
                        }
                        let (conv_num, _, _, _, _) = empties[i];
                        ub += conv_num / civ_den;
                        empties[i].3 -= 1;
                        conv_done += 1;
                        civ_den = (civ_den - 1.0).max(1.0);
                    }
                    need -= conv_done;
                }

                if need == 0 {
                    return ub;
                }

                // Build military stage
                let post_civ_den = (total_civ - conv_cap).max(1) as f64;
                let mut builds: Vec<(f64, i32)> = empties.iter().map(|t| (t.1, t.4)).collect();
                builds.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
                for (build_num, cap) in builds {
                    if need == 0 || cap <= 0 {
                        break;
                    }
                    let take = cap.min(need);
                    ub += (take as f64) * (build_num / post_civ_den);
                    need -= take;
                }
                if need > 0 { f64::INFINITY } else { ub }
            }
            TargetType::Civilian => {
                // Build civilian factories only
                let civ_den = total_civ.max(1) as f64;
                let mut builds: Vec<(f64, i32)> =
                    empties_per_node.iter().map(|t| (t.2, t.4)).collect();
                builds.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
                let mut ub = 0.0f64;
                for (build_num, cap) in builds {
                    if need == 0 || cap <= 0 {
                        break;
                    }
                    let take = cap.min(need);
                    ub += (take as f64) * (build_num / civ_den);
                    need -= take;
                }
                if need > 0 { f64::INFINITY } else { ub }
            }
            TargetType::Factories => {
                // For total factories, conversions don't change the count, so we can't use conversions
                // to reach the target. We can only build new factories.
                // Optimize: build cheapest factories (military is cheaper than civilian at 7200 vs 10800)
                let civ_den = total_civ.max(1) as f64;
                // Sort by military cost (cheaper), use all empty slots for military first
                let mut builds: Vec<(f64, i32)> =
                    empties_per_node.iter().map(|t| (t.1, t.4)).collect();
                builds.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
                let mut ub = 0.0f64;
                for (build_num, cap) in builds {
                    if need == 0 || cap <= 0 {
                        break;
                    }
                    let take = cap.min(need);
                    ub += (take as f64) * (build_num / civ_den);
                    need -= take;
                }
                if need > 0 { f64::INFINITY } else { ub }
            }
        }
    }

    fn name(&self) -> &'static str {
        "best_infra_upper_bound"
    }
}
