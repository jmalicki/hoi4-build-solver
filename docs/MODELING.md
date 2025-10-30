### Modeling details

- **States**: A state encodes all nodes’ (`numInfra`, `numCivilian`, `numMilitary`). `numSlots` is per-node fixed input and not part of the dynamic state. `totalCost` is accumulated via per-action costs and is not a state variable. Terminal states satisfy `sum(numMilitary over nodes) = targetMilitary`.
  - Value ranges used by the implementation: `numInfra ∈ [0,5]`, `numCivilian ∈ [0,255]`, `numMilitary ∈ [0,255]`. The sum constraint `numCivilian + numMilitary ≤ numSlots` always holds.

- **Actions**: For each time step, pick one node and one of four actions, subject to feasibility:
  - `civilian(node)`: valid if `numCivilian + numMilitary < numSlots`.
  - `military(node)`: valid if `numCivilian + numMilitary < numSlots`.
  - `infra(node)`: valid if `numInfra < 5`.
  - `convert(node)`: valid if `numCivilian ≥ 1` and capacity after conversion stays within `numSlots`.

- **Transitions**: Deterministic; only one node changes by ±1 per step. Successors are generated on-demand during search.

- **Costs**: For node `i` with current `numInfra = k`, define `infraMultiplier = 1 + (2k)/10` and let `sumCivilian = Σ_j numCivilian[j]` over all nodes in the current state (using `max(1, sumCivilian)` to avoid div-by-zero). Immediate action costs are:
  - civilian: `civilianCost / infraMultiplier / sumCivilian`
  - military: `militaryCost / infraMultiplier / sumCivilian`
  - infra: `infraCost / infraMultiplier / sumCivilian`
  - convert: `conversionCost / infraMultiplier / sumCivilian`

### A* heuristic

- Let `remainingMil = max(0, targetMilitary - sum(numMilitary))`.
- Define an optimistic per-military cost under two optimistic assumptions:
  1) Best infrastructure multiplier: behave as if `numInfra = 5` on the acting node.
  2) Largest feasible civilian denominator: use an upper bound on civilians attainable during the remaining build. Let
     - `sumCivil = Σ_j numCivilian[j]` (current civilians),
     - `empty = Σ_j max(0, numSlots[j] − numCivilian[j] − numMilitary[j])` (currently empty slots),
     - `remainingMil = max(0, targetMilitary − Σ_j numMilitary[j])`.
     Civilians can fill at most `max(0, empty − remainingMil)` of the empties (the rest must become military). Define `civUpper = max(1, sumCivil + max(0, empty − remainingMil))` and divide costs by this.

- Let `bestUnitBase = min(militaryCost, conversionCost)` and `bestMult = 1 + (2·5)/10`.
- Per-unit optimistic cost: `bestUnitCost = (bestUnitBase / bestMult) / civUpper`.
- Heuristic: `h(s) = remainingMil * bestUnitCost`.

Properties:
- **Admissible**: Actual step cost uses denominator `sumCivilian(state)` and current infra; our heuristic uses the best (smallest) possible numerator and the largest feasible denominator `civUpper`. Thus each step’s actual cost is ≥ the per-unit term used in `h`, so `h` never overestimates remaining cost.
- **Consistent**: `h(goal)=0`. For actions adding one military (military/convert), `h` decreases by at most one `bestUnitCost`, while the true step cost is ≥ `bestUnitCost`, so `h(s) − h(s') ≤ c(s,a)`. For actions that do not add military (infra/civilian), `remainingMil` is unchanged so `h(s)=h(s')`, and step cost `c(s,a) > 0`, hence `h(s) ≤ c(s,a) + h(s')`. Therefore `h` is consistent and A* does not need re-expansions under a best‐g closed policy.

Refinements (optional):
- Add a lower bound on infra costs necessary to achieve the assumed multiplier (e.g., the minimal sequence to reach `infra=5` on cheapest nodes). Maintain consistency by subtracting at most one lower-bound “step cost” per required infra increment.
- Similarly, include a lower bound on additional civilians to approach the optimistic denominator, but cap decreases in `h` per action by a per-step lower bound so `h(s) ≤ c(s,a) + h(s')` remains true.


