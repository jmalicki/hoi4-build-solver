# Heuristics Module

This module implements pluggable heuristics for the HOI4 MDP solver. Each heuristic provides both:
- **Lower bound** (`lower_bound()`): An admissible heuristic `h(s)` for A* search
- **Upper bound** (`upper_bound()`): An upper bound on remaining cost for pruning

## Theoretical Background

### Admissibility

A heuristic `h(s)` is **admissible** if it never overestimates the true optimal cost from state `s` to the goal:
```
h(s) ≤ h*(s)  for all states s
```

where `h*(s)` is the true optimal cost from `s` to the goal.

**Why it matters**: An admissible heuristic guarantees A* will find optimal solutions (if a solution exists).

### Consistency (Monotonicity)

A heuristic `h(s)` is **consistent** (or monotonic) if:
```
h(s) ≤ c(s,a) + h(s')  for all states s, actions a, successors s'
```

where `c(s,a)` is the cost of action `a` from state `s`.

**Why it matters**: A consistent heuristic ensures A* never needs to re-expand states (under a best-g closed policy), improving efficiency.

## Available Heuristics

### `BestInfraUpperBoundHeuristic`

**Full name**: Best-case infrastructure with upper-bound civilians heuristic

**Key assumptions**:
1. **Best-case infrastructure**: Assumes the acting node will have `infra=5` (maximum multiplier)
2. **Upper-bound civilians**: Uses an optimistic upper bound on the denominator (civilians) during remaining construction

#### Lower Bound

The admissible lower bound `h(s)` is computed as follows:

**For Military targets:**
1. Compute `remaining = max(0, target - sum(military))`
2. If `remaining == 0`, return `0.0` (already at goal)
3. Compute optimistic cost per unit:
   - Best infra multiplier: `best_mult = 1 + (2·5)/10 = 2.0`
   - Upper bound on civilians: `civUpper = max(1, sumCivil + max(0, empty - remaining))`
     - Where `sumCivil = Σ numCivilian[j]`
     - And `empty = Σ max(0, numSlots[j] - numCivilian[j] - numMilitary[j])`
   - Blended base cost: Mix of conversion (4000 base) and military build (7200 base)
     - `conv_usable = min(remaining, sumCivil)` (can convert at most this many)
     - `mil_needed = remaining - conv_usable` (rest must be built)
     - `blended_base = 4000·conv_usable + 7200·mil_needed`
4. Per-unit cost: `blended_base / best_mult / civUpper`
5. Total: `h(s) = remaining · per_unit_cost`

**For Civilian targets:**
- Similar structure, but only building is possible (no conversions)
- Base cost: `10800` per civilian factory
- `h(s) = remaining · (10800 / best_mult / civUpper)`

**For Total Factories targets:**
- Conversions don't change factory count, so only building matters
- Use cheapest build option: military at `7200` base (cheaper than civilian at `10800`)
- `h(s) = remaining · (7200 / best_mult / civUpper)`

**Proof of Admissibility**:

The heuristic never overestimates because:
1. **Infrastructure**: Actual infra ≤ 5, so actual multiplier ≥ best_mult (infra multiplier is in denominator)
2. **Civilians**: Actual `sumCivilian` during execution ≤ `civUpper` (civilians in denominator)
3. **Base costs**: Actual actions use the same or higher base costs than assumed
4. **Conversions**: Actual conversion opportunities ≤ assumed `conv_usable`

Therefore, each step's actual cost ≥ the per-unit term used in `h`, so `h(s) ≤ h*(s)`.

**Proof of Consistency**:

For actions that add one military (military/build or convert):
- `h(s)` decreases by at most one `per_unit_cost`
- True step cost `c(s,a) ≥ per_unit_cost` (actual infra ≤ best, actual civ ≤ civUpper)
- So `h(s) - h(s') ≤ per_unit_cost ≤ c(s,a)`, hence `h(s) ≤ c(s,a) + h(s')`

For actions that don't change the target count (infra, civilian):
- `h(s) = h(s')` (remaining unchanged)
- `c(s,a) > 0`
- So `h(s) = h(s') ≤ c(s,a) + h(s')`

Therefore, the heuristic is consistent.

#### Upper Bound

The upper bound uses a greedy "convert then build" strategy:

**For Military targets:**
1. **Conversion stage**: Convert civilians to military, choosing cheapest nodes first
   - Sort nodes by `4000 / infra_mult(infra)` (conversion cost per unit)
   - Convert up to `min(remaining, total_civilians)`
   - Denominator decreases by 1 per conversion: `civ_den = max(1, total_civ - conversions_done)`
2. **Build stage**: Build military factories on remaining empty slots
   - Sort nodes by `7200 / infra_mult(infra)` (military build cost per unit)
   - Allocate remaining need to cheapest nodes, limited by empty slots
   - Use constant denominator: `post_civ_den = max(1, total_civ - conversions_cap)`
3. If total capacity is insufficient, return `f64::INFINITY`

**For Civilian targets:**
- Build civilian factories only (no conversions applicable)
- Sort by `10800 / infra_mult(infra)`
- Allocate to cheapest nodes, limited by empty slots

**For Total Factories targets:**
- Conversions don't change count, so only building matters
- Use cheapest option: military at `7200` base
- Sort by `7200 / infra_mult(infra)`
- Allocate to cheapest nodes, limited by empty slots

**Use for pruning**: If `g(s) + ub(s) > best_known_solution_cost`, we can safely prune state `s` (no optimal path goes through it).

### `ZeroHeuristic` (string name: `djikstra`)

- `lower_bound(s) = 0` for all states `s` (admissible but weakest possible)
- `upper_bound(s) = +∞` (disables pruning)
- Reduces A* to classic Dijkstra's algorithm (uniform-cost search)

Use when you want correctness without any heuristic guidance, e.g., to debug heuristic behavior or as a baseline.

## Future Heuristics

Future implementations might include:
- **Zero heuristic**: Always returns 0 (Dijkstra's algorithm - admissible but weak)
- **Greedy heuristic**: Optimistic estimate based on cheapest single action
- **Relaxed heuristic**: Uses relaxed problem constraints (e.g., ignore infra costs)
- **Learning heuristic**: Uses learned cost estimates from previous runs

## References

- Russell & Norvig, *Artificial Intelligence: A Modern Approach* (3rd ed.), Chapter 3.6 "Informed Search Strategies"
- Hart, Nilsson, & Raphael, "A Formal Basis for the Heuristic Determination of Minimum Cost Paths" (1968)
- Wikipedia: [A* search algorithm](https://en.wikipedia.org/wiki/A*_search_algorithm)
- Wikipedia: [Admissible heuristic](https://en.wikipedia.org/wiki/Admissible_heuristic)
- Wikipedia: [Consistent heuristic](https://en.wikipedia.org/wiki/Consistent_heuristic)
- Red Blob Games: [Introduction to A*](https://www.redblobgames.com/pathfinding/a-star/introduction.html)
- Stanford CS221 Notes: [Informed Search](https://stanford-cs221.github.io/spring2023/notes/search)
