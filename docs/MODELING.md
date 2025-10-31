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

**See `rust/hoi4_mdp_core/src/heuristic/README.md` for detailed heuristic descriptions, theoretical background, and proofs of admissibility/consistency.**

The solver uses pluggable heuristics via the `Heuristic` trait. The default heuristic (`BestInfraUpperBoundHeuristic`) provides:

- **Admissible lower bound**: An optimistic estimate using best-case infrastructure (infra=5) and an upper bound on civilians (`civUpper`) as the denominator
- **Upper bound**: A greedy "convert then build" strategy used for pruning

For complete details, implementation notes, and theoretical proofs, see the heuristic module README.


