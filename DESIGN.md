### Problem in plain words

We have several named nodes. Each node tracks three changing state variables:
- **numInfra**: infrastructure level per node in [0, 5]
- **numCivilian**: number of civilian factories on the node (0 ≤ numCivilian ≤ numSlots)
- **numMilitary**: number of military factories on the node (0 ≤ numMilitary ≤ numSlots)

Each node also has an unchanging state parameter:
- **numSlots**: capacity for factories on the node

There is a global running **totalCost** that starts at 0. Across all nodes, we must always satisfy the constraint: **numMilitary + numCivilian ≤ numSlots** for each node.

At each time step, we choose exactly one node and do one of four actions:
- **civilian**: add 1 civilian on that node, cost = civilianCost / (1 + (2 · numInfra) / 10)
- **military**: add 1 military on that node, cost = militaryCost / (1 + (2 · numInfra) / 10)
- **infra**: add 1 infrastructure on that node, cost = infraCost / (1 + (2 · numInfra) / 10)
- **convert**: convert 1 civilian → 1 military on that node (keeping civilian ≥ 0), cost = conversionCost / (1 + (2 · numInfra) / 10)

Fixed costs:
- **infraCost = 6000**
- **civilianCost = 10800**
- **militaryCost = 7200**
- **conversionCost = 4000**

The goal is to reach a state where the sum of military factories over all nodes equals a given **targetMilitary**, while minimizing **totalCost**. The program will take the initial list of nodes and their values, and produce:
- A sequence of moves like `(nodeName, "military")`, `(nodeName, "civilian")`, `(nodeName, "infra")`, `(nodeName, "convert")`
- The final state in the same structure as the input

### Libraries and approach

- We use an on-demand graph search with A* over the implicit state space to avoid enumerating all states.
- Performance core will be implemented in **Rust** and exposed to Python via **PyO3**. Python remains the CLI/IO and orchestration layer; Rust handles state encoding, successor generation, heuristic, and the A* loop.

### Modeling details

- **States**: A state encodes all nodes’ (`numInfra`, `numCivilian`, `numMilitary`) values. `numSlots` is per-node fixed input and not part of the dynamic state. `totalCost` is not part of the state (it is accumulated via per-action costs as negative rewards). The terminal states satisfy `sum(numMilitary over nodes) = targetMilitary` (we accept any terminal with minimal cost).

- **Actions**: For each time step, the action set is the union across nodes of four choices, but feasibility depends on the current node’s values:
  - `civilian(node)` valid if `numCivilian + numMilitary < numSlots`.
  - `military(node)` valid if `numCivilian + numMilitary < numSlots`.
  - `infra(node)` valid if `numInfra < 5`.
  - `convert(node)` valid if `numCivilian ≥ 1` and `numMilitary + numCivilian ≤ numSlots` after conversion.

- **Transitions**: Deterministic. Each valid action updates exactly one node’s variables by ±1 and leaves other nodes unchanged. In A*, we generate successors on demand; goal detection ends the search.

- **Action cost**: Let `i` be the chosen node and `k = numInfra[i]` before the action. Define `infraMultiplier = 1 + (2 * k) / 10` and let `sumCivilian = Σ_j numCivilian[j]` over all nodes in the current state. The immediate cost of an action is the base cost divided by both multipliers:
  - civilian: `civilianCost / infraMultiplier / sumCivilian`
  - military: `militaryCost / infraMultiplier / sumCivilian`
  - infra: `infraCost / infraMultiplier / sumCivilian`
  - convert: `conversionCost / infraMultiplier / sumCivilian`
  - Note: To avoid division-by-zero, we define `sumCivilian >= 1` (i.e., use `max(1, sumCivilian)`).

- **Objective**: Reach any state with `sum(numMilitary)=targetMilitary` at minimal cumulative cost. We solve a deterministic shortest-path over the implicit state graph using A*.

### A* heuristic

- **Choice**: Use A*. Dijkstra is A* with `h(s)=0`. With an admissible, consistent heuristic, A* explores no more nodes than Dijkstra and often far fewer.
- **Heuristic (admissible and consistent)**: Let `remainingMil` be max(0, `targetMilitary - sum(numMilitary)`). Define a per-unit optimistic cost using the best multiplier (assume infra at 5) and the cheapest path to 1 military on a slot (direct military vs convert). Then
  - `h(s) = remainingMil * bestUnitCostWithMaxMultiplier`.
  - Consistency sketch: actions either reduce `remainingMil` by 1 (h drops by ≤ that lower bound, while action cost ≥ that bound) or leave it unchanged (h constant, action cost > 0), so `h(s) ≤ c(s,a) + h(s')` and `h(goal)=0`.
- **Refinements**: Optionally add a lower bound on infra costs needed to reach the assumed multiplier; maintain consistency by subtracting at most per-step lower bounds.

### Solution approach

- Rust crate `hoi4_mdp_core` (lib):
  - Encodes state as a compact struct (per-node `(infra,civ,mil)` with `numSlots` from inputs) and provides:
    - `iter_successors(state) -> iterator` yielding feasible successors and costs without heap allocations.
    - `heuristic(state) -> f64` per docs/MODELING.md (best-case infra, `civUpper = civ + max(0, empty - remainingMil)`).
    - `solve_a_star(init_state, target) -> (goal_state, total_cost, parent_map)` with periodic progress callbacks (iters, heap size).
    - Path reconstruction utility from `parent_map`.
  - Exposed via PyO3 functions/classes mirroring the current Python API.
- Python module `mdp_solver` calls into the Rust functions, handles CSV/Sheets IO, and writes `moves.csv`/`final_state.csv`.

### Practical considerations

- **State explosion**: Avoid full enumeration by using on-demand A* search only; Rust iterator-based successors minimize allocations.
- **Determinism**: Transitions are deterministic; each action yields exactly one successor.
- **Goal handling**: Stop search when `sum(numMilitary)=targetMilitary`.
- **Validation**: Enforce feasibility constraints when generating successors.

### Rust/PyO3 interface

- Packaging: Rust crate compiled as a Python extension module via maturin/uv integration.
- API surface (Python):
  - `solve_a_star(nodes: List[Node], target: int, verbose: bool, print_every: int) -> (goal_state, total_cost, parent)`
  - `reconstruct_moves(parent, goal_state, nodes) -> List[(nodeName, actionStr)]`
- Progress: optional callback hook `on_progress(iters, g, steps, heap_len)` from Rust; default prints match current Python output.

### Migration plan

1. Create Rust crate `hoi4_mdp_core` with PyO3 bindings that match current Python signatures.
2. Port: state struct, cost model, successor generator, heuristic, A* loop, path reconstruction.
3. Replace Python implementations with Rust calls, keeping CLI unchanged.
4. Validate parity against current Python solver using the same inputs; compare totalCost and moves length.

### Output format

- **Plan**: A sequence like `(nodeA, "military"), (nodeB, "infra"), ...` derived by simulating the optimal policy from the initial state.
- **Final state**: Same structure as input nodes, updated after applying the plan.

### Next steps

- Implement A* search with the admissible, consistent heuristic in Python.
- Optionally port the hot loop to C++20 later if needed; keep Python CLI/IO.

### I/O and preprocessing

- CLI accepts either `--input nodes.csv` or `--sheet-url` (Google Sheets tab URL). The sheet URL is converted to a CSV export of the active tab.
- Columns: `nodeName,numSlots,numInfra,numCivilian,numMilitary` with optional `Docks,Refineries` (subtracted from `numSlots`). Aliases are detected (e.g., `slots`, `infra`, `civilian`, `military`, `dockyards`).

### Implementation targets

- Python reference implementation for correctness and CLI.
- Performance-sensitive components to be implemented in C++20, exposed to Python (e.g., pybind11), and optionally leveraging AI-Toolbox.


