# Parallel Construction Design

## Overview

The current MDP solver approximates construction by applying one action per timestep. This design extends the model to
handle parallel construction, where multiple projects can proceed simultaneously subject to factory allocation
constraints.

## Core Constraint

**15 civilian factories per construction project**: Each active construction project can use up to 15 civilian
factories. Projects are processed in FIFO order from a global construction queue.

## State Representation

### Global State Extensions

The `State` structure is extended with:

```rust
pub struct State {
    nodes: Vec<NodeState>,  // Existing per-node state
    construction_queue: ConstructionQueue,  // NEW: Global construction queue (encapsulated)
    time_elapsed: f64,  // NEW: Cumulative time cost (replaces totalCost sum)
}
```

### Construction Queue Item

```rust
struct ConstructionItem {
    node_index: usize,      // Which node this construction affects
    action_type: Action,    // "civilian", "military", "infra", "convert"
    cost_remaining: f64,    // Base cost remaining (raw cost, before
                            // infra/factory allocation). Examples:
                            // 7200 (military), 10800 (civilian),
                            // 4000 (convert), 6000 (infra)
// Formula: effective_time = cost_remaining / (infra_multiplier * factories_allocated)
}
```

**Both factory allocation and infra multiplier are implicit**: When processing the queue:

- **Factory allocation**: Computed in FIFO order (first item gets min(15, remaining_factories), etc.)
- **Infra multiplier**: Computed from current node infra level at the moment of calculation
- **Effective completion time**:
  $\text{effective\_time} =
  \dfrac{\text{cost\_remaining}}{\text{current\_infra\_mult}
  \times\;\text{factories\_allocated}}$

This makes both factors implicit:

- Factory allocation changes as queue changes (FIFO order)
- Infra multiplier changes when infra completes (use current level, no adjustment needed)
- When advancing time by $\delta_t$:
  $\text{cost\_remaining} \mathrel{-=} \delta_t \times
  \text{current\_infra\_mult} \times\;\text{factories\_allocated}$
- Items with `cost_remaining <= 0` are completed

**Benefits**: No need to adjust `cost_remaining` when infra changes - we just use the new infra_multiplier in future
calculations.

### Per-Node Construction Tracking

Extend `NodeState` with:

```rust
struct NodeState {
    infra: u8,
    civ: u8,
    mil: u8,
    under_construction: u8,  // NEW: Number of items in queue affecting this node
}
```

Alternatively, keep a separate map (lighter weight if most nodes have 0):

```rust
under_construction: HashMap<usize, u8>,  // node_index -> count
// Absence implies 0
```

## Successor Generation

### Phase 1: Add New Construction Item (Optional)

When generating successors from a state:

- Optionally add one new item to the construction queue (subject to slot capacity)
- This represents the decision to start a new construction project
- Each successor adds a different construction item, or none at all

**Constraints**:

- Cannot exceed slot capacity (civ + mil + under_construction < slots)
- For conversions: requires existing civilian factory (civ > 0)

### Phase 2: Time Advancement (If Factories Fully Utilized)

After adding a construction item, check factory allocation:

1. **Sort queue by age** (FIFO - already a deque)
2. **Allocate factories** to queue items (up to 15 each):
   - First item gets min(15, remaining_factories)
   - Continue until factories exhausted or queue exhausted
   - Remaining factories = total_civ - (15 \* items_with_15_factories)

3. **If factories are fully utilized** (all queue items have 15 factories, or queue empty):
   - Allocate factories in FIFO order: first item gets min(15, total_civ), second gets min(15, total_civ -
     first_allocation), etc.
   - For each item, compute effective completion time using **current** infra level at that node:

- $\text{effective\_time} =
   \dfrac{\text{cost\_remaining}}{\text{current\_infra\_mult}
   \times\;\text{factories\_allocated}}$
- Find the item with minimum `effective_time` (next to complete)
- Let $\delta_t$ be that minimum effective time (ensures no $\text{cost\_remaining}$ goes negative)
- For each item, reduce its $\text{cost\_remaining}$ by
  $\delta_t \times \text{current\_infra\_mult} \times
   \text{factories\_allocated}$ (time advances for all)
- **Invariant**: $\text{cost\_remaining} \ge 0$ (at least one item will have $\text{cost\_remaining} = 0$, others remain
  positive)
  - Complete items where `cost_remaining == 0` (exactly the minimum item), updating node state
  - **Note**: When infra completes, future calculations for items on that node automatically use the new
    infra_multiplier - no adjustment needed
  - Remove completed items from queue

1. **If factories are under-utilized** (remaining_factories > 0 after allocation):
   - Do not advance time
   - Enqueue this successor state as-is
   - The next decision point will use the unused factories

### Successor API

```rust
fn iter_successors(state: &State, nodes: &[NodeDesc]) -> impl Iterator<Item = Successor>
```

Each `Successor` contains:

- `next_state: State` - the new state after adding a construction item (and possibly advancing time)
- `time_increment: f64` - time that passed in this transition (0 if no advancement)
- `action: Action` - the construction item that was added
- `node_index: usize` - which node was acted upon

## Time vs Cost

The existing "cost" formula is actually time:

- $\dfrac{\text{base\_cost}}{\text{infra\_multiplier} \times
   \text{total\_civilian}}$
- This represents time to complete given factory allocation

For the construction queue:

- $\text{cost\_remaining}$ = raw base cost (7200 for military, 10800 for civilian, etc.)
- Both $\text{infra\_multiplier}$ and $\text{factories\_allocated}$ are computed from current state when needed
- Effective completion time:
  $\text{effective\_time} =
  \dfrac{\text{cost\_remaining}}{\text{infra\_mult}
  \times\;\text{factories\_allocated}}$
- When advancing time:
  - $\delta_t =
    \min\left(\dfrac{\text{cost\_remaining}}{\text{infra\_mult}
    \times\;\text{factories\_allocated}}\right)$
    across all queue items
  - For each item:
    $\text{cost\_remaining} = \text{cost\_remaining} -\left(\delta_t
    \times\;\text{current\_infra\_mult}
    \times\;\text{factories\_allocated}\right)$
  - **Invariant**: $\text{cost\_remaining} \ge 0$ always ($\delta_t$ is chosen to ensure this)
  - Items with $\text{cost\_remaining} = 0$ are completed (exactly those that determined $\delta_t$)
- **Infra changes are implicit**: When infra completes, future calculations for items on that node automatically use the
  new infra_multiplier

**Example**: Two military factories in queue, both with cost_remaining=7200, infra_mult=1.0 (infra level 0):

- Item 1: allocated 15 factories → $\text{effective\_time} = \dfrac{7200}{1.0 \times 15} = 480$ days
- Item 2: allocated 10 factories → $\text{effective\_time} = \dfrac{7200}{1.0 \times 10} = 720$ days
- `delta_t = min(480, 720) = 480` days
- After 480 days:
  - Item 1: `cost_remaining = 7200 - (480 * 1.0 * 15) = 0` → **completed**
  - Item 2: `cost_remaining = 7200 - (480 * 1.0 * 10) = 2400` → still under construction
- If infra upgrades to level 5 (mult=2.0) on Item 2's node while it's building:
  - Next calculation uses new mult: `effective_time = 2400 / (2.0 * 10) = 120` days (faster! Higher mult = faster)

## State Pool Integration

The state pool and A\* search remain largely unchanged:

- States are still hashed/compared by their content
- The construction queue is part of state identity
- `g` value is now cumulative time (not cost sum)
- Heuristic `h(s)` estimates remaining time to goal

## API Design

### Construction Queue Struct

The construction queue is managed by a separate struct for better encapsulation:

```rust
struct ConstructionQueue {
    items: VecDeque<ConstructionItem>,
}

impl ConstructionQueue {
    /// Create empty queue
    fn new() -> Self;

    /// Add construction item to queue (if capacity allows)
    /// Stores raw base_cost (7200 for military, etc.) - infra/factory
    /// allocation computed later
    fn try_add(&self, node_idx: usize, action: Action, base_cost: f64) -> Option<Self>;

    /// Allocate factories to queue items in FIFO order (computed on-demand)
    /// Uses current total_civilian from state
    /// Returns Vec<(item_index, factories_allocated)>
    fn allocate_factories(&self, total_civilian: u32) -> Vec<(usize, u8)>;

    /// Advance time to next completion, updating queue and returning
    /// completed items
    /// Factory allocation and infra_multiplier are recomputed from current
    /// state (both implicit)
    /// delta_t = min(cost_remaining / (infra_mult * factories_allocated))
    /// across all items
    /// Maintains invariant: cost_remaining >= 0 for all items
    fn advance_to_next_completion(
        &mut self,
        nodes: &[NodeState],
        node_descs: &[NodeDesc],
    ) -> (f64, Vec<CompletedItem>);
    // Returns (time_elapsed, completed_items)

    /// Check if factories are fully utilized
    fn factories_fully_utilized(&self, total_civilian: u32) -> bool;

    /// Get current queue length
    fn len(&self) -> usize;

    /// Check if queue is empty
    fn is_empty(&self) -> bool;
}

struct CompletedItem {
    node_index: usize,
    action: Action,
}
```

### State Construction

```rust
impl State {
    /// Create initial state from node configurations
    fn new(nodes: Vec<NodeState>) -> Self;

    /// Access construction queue
    fn construction_queue(&self) -> &ConstructionQueue;

    /// Access construction queue mutably
    fn construction_queue_mut(&mut self) -> &mut ConstructionQueue;
}
```

### Successor Generation Implementation

```rust
fn iter_successors(
    state: &State,
    nodes: &[NodeDesc],
) -> impl Iterator<Item = Successor> {
    // Phase 1: Generate states with new construction items added
    let base_successors = generate_construction_items(state, nodes);

    // Phase 2: For each, conditionally advance time
    base_successors.flat_map(move |mut succ| {
        let total_civ = succ.next_state.total_civilian();
        if succ
            .next_state
            .construction_queue()
            .factories_fully_utilized(total_civ)
        {
            let mut new_state = succ.next_state;
            let (delta_t, completed_items) = new_state.construction_queue_mut()
                .advance_to_next_completion(&new_state.nodes, nodes);
            // Apply completed items to update node state
            for completed in completed_items {
                apply_completion(&mut new_state, completed);
            }
            Some(Successor {
                next_state: new_state,
                time_increment: succ.time_increment + delta_t,
                action: succ.action,
                node_index: succ.node_index,
            })
        } else {
            Some(succ) // No time advancement, factories under-utilized
        }
    })
}
```

### Transition Cost

The step cost for A\* is the `time_increment` from the successor:

- If no time advancement: `time_increment = 0.0` (decision state, no cost)
- If time advancement: `time_increment = delta_t` (actual time elapsed)

## Output Format

The `moves.csv` output format remains the same as before - each action represents adding a new item to the construction
queue:

- `(nodeName, "military")` - enqueue military factory construction
- `(nodeName, "civilian")` - enqueue civilian factory construction
- `(nodeName, "infra")` - enqueue infrastructure upgrade
- `(nodeName, "convert")` - enqueue conversion (civilian → military)

**Semantic change**: Each action now means "enqueue this construction item" rather than "complete this action
immediately". The actions appear the same in `moves.csv`, but the underlying model is different - construction happens
in parallel with factory allocation, and items may complete at different times.

**Time advancement is implicit**: When factories are fully utilized and time advances to the next completion, these
internal state transitions are not shown as separate moves. Only the enqueue actions appear.

**Example**: The moves.csv looks identical to before:

- `(A, "military")`
- `(B, "military")`
- `(C, "military")`

But internally, between each enqueue, time may advance automatically and items may complete, updating the state
accordingly. These intermediate transitions are compressed - the user only sees the sequence of enqueue decisions.

## Heuristic Updates

The heuristic `h(s)` needs to account for the construction queue:

- Items already in queue will complete in some time (given factory allocation)
- Remaining work beyond the queue still uses the existing heuristic formula
- Heuristic = `max(queue_completion_time, remaining_work_heuristic)`

## Benefits

1. **More accurate**: Models actual parallel construction behavior
2. **Fewer states**: Decision points are when factories become available, not every timestep
3. **Better optimization**: Can see benefits of starting multiple projects early

## Design Decisions

1. **No queue removal mid-construction**: Once a construction item is added to the queue, it must complete. This
   simplifies state space and avoids cancellation logic.

2. **Infra completing mid-construction**: Since we store `cost_remaining` (raw cost) and compute infra_multiplier from
   current node state, infra changes are automatically handled:
   - When infra completes, the node's infra level increases
   - Future calculations for items on that node automatically use the new (higher) infra_multiplier
   - No adjustment to `cost_remaining` needed - the better multiplier naturally speeds up remaining work
   - Example: Item has 2400 cost_remaining, infra upgrades from level 0 (mult=1.0) to level 5 (mult=2.0):
     - Before: `effective_time = 2400 / (1.0 * 10) = 240` days
     - After: `effective_time = 2400 / (2.0 * 10) = 120` days (faster! Higher multiplier = faster completion)

3. **FIFO factory allocation**: Factories are allocated in queue order (first-in, first-out). This is the simplest model
   and matches typical game behavior. Optimizing allocation order would require solving a harder subproblem and is
   deferred.
