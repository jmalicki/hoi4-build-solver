# HOI4 Build Solver

[![CI](https://github.com/jmalicki/hoi4-build-solver/actions/workflows/ci.yml/badge.svg)](https://github.com/jmalicki/hoi4-build-solver/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![CodeRabbit](https://img.shields.io/badge/CodeRabbit-Enabled-brightgreen)](https://coderabbit.ai)

An optimal build planner for Hearts of Iron 4 that finds the minimal-cost sequence of construction actions to reach
target factory counts. Uses A\* search with admissible heuristics to solve the optimization problem without enumerating
the full state space.

## Problem Overview

In Hearts of Iron 4, you manage industrial construction across multiple nodes (states/provinces). Each node has:

- **Infrastructure** (0-5): Multiplies construction speed
- **Civilian factories**: Used for construction and trade
- **Military factories**: Used for equipment production
- **Building slots**: Total capacity for factories

You can perform four types of actions per node:

1. **Build civilian factory**: Costs civilianCost / (infraMultiplier × totalCivilians)
2. **Build military factory**: Costs militaryCost / (infraMultiplier × totalCivilians)
3. **Build infrastructure**: Costs infraCost / (infraMultiplier × totalCivilians)
4. **Convert civilian → military**: Costs conversionCost / (infraMultiplier × totalCivilians)

The cost model accounts for:

- **Infrastructure multiplier**: `1 + (2 × infraLevel) / 10` (higher infra = faster construction)
- **Factory allocation**: Costs scale inversely with total civilian factories (more factories = faster construction)
- **Parallel construction**: Multiple projects can proceed simultaneously, each using up to 15 civilian factories

The solver finds the optimal sequence of actions to reach a target count (military factories, civilian factories, or
total factories) while minimizing total construction time.

## Architecture

- **Core**: Rust implementation using A\* search with pluggable heuristics
- **Interface**: Python CLI via PyO3 bindings
- **Search**: A\* over implicit state space (no full enumeration)
- **Heuristics**: Admissible, consistent lower bounds for optimal search
- **Pruning**: Upper-bound pruning to skip non-optimal branches
- **Parallel construction**: Models simultaneous projects with factory allocation

See `docs/DESIGN.md` for detailed architecture and modeling decisions.

## Setup

Prerequisites: Python 3.10+ and `uv` installed. Install uv from
`https://docs.astral.sh/uv/getting-started/installation/`.

Create and activate a virtualenv managed by uv:

```bash
uv venv .venv
source .venv/bin/activate
uv sync
```

This installs dependencies from `pyproject.toml` into `.venv`.

## Input Format

Input CSV must have columns:

- `nodeName` (str): Name/identifier of the node
- `numSlots` (int ≥ 0): Total building slots (capacity)
- `numInfra` (int in [0,5]): Current infrastructure level
- `numCivilian` (int ≥ 0): Current civilian factories
- `numMilitary` (int ≥ 0): Current military factories

Optional columns (if present, they are subtracted from numSlots before modeling):

- `Docks` (int ≥ 0): Naval dockyards (reduce available slots)
- `Refineries` (int ≥ 0): Synthetic refineries (reduce available slots)

Constraint per node: `numMilitary + numCivilian ≤ numSlots`.

Column name aliases are supported (e.g., `slots`, `infra`, `civ`, `mil`, `dockyards`).

## Usage

### Basic Usage

```bash
hoi4-build-solve \
  --input nodes.csv \
  --target-type military \
  --target 30 \
  --moves-out moves.csv \
  --final-out final_state.csv
```

### Command-Line Options

- `--input`: Path to input CSV file
- `--sheet-url`: Google Sheet URL (uses CSV export of the active tab) - provide exactly one of `--input` or
  `--sheet-url`
- `--target-type`: Type of target - `military` (military factories), `civilian` (civilian factories), or `factories`
  (total factories)
- `--target`: Target value for the selected target type
- `--moves-out`: Output CSV path for the action sequence
- `--final-out`: Output CSV path for the final node states
- `--heuristic`: Heuristic to use - `best_infra_upper_bound` (default), `standard` (alias), or `zero`/`dijkstra` (no
  heuristic)
- `--no-prune`: Disable upper-bound pruning (pruning enabled by default)
- `--verbose`/`--quiet`: Control progress output (default: verbose)
- `--print-every`: Print progress every N iterations (default: 10000, set to 0 to disable)

### Google Sheets Input

Read directly from a Google Sheet:

```bash
hoi4-build-solve \
  --sheet-url "https://docs.google.com/spreadsheets/d/.../edit?gid=1859149470#gid=1859149470" \
  --target-type military \
  --target 30 \
  --moves-out moves.csv \
  --final-out final_state.csv
```

## Output

The solver produces two CSV files:

1. **Moves CSV** (`--moves-out`): Sequence of actions to execute
   - Columns: `step`, `nodeName`, `action` (`civilian`, `military`, `infra`, or `convert`)
   - Actions are ordered chronologically and can be executed sequentially

2. **Final State CSV** (`--final-out`): Final state of all nodes after applying all moves
   - Same schema as input: `nodeName`, `numSlots`, `numInfra`, `numCivilian`, `numMilitary`
   - Useful for verifying the solution reached the target

## Features

- **Optimal solutions**: Uses A\* search with admissible heuristics to guarantee optimality
- **Parallel construction modeling**: Accurately models simultaneous construction projects with factory allocation
- **Multiple target types**: Can optimize for military factories, civilian factories, or total factories
- **Pluggable heuristics**: Choose from different heuristic strategies for different search characteristics
- **Upper-bound pruning**: Skips non-optimal branches to improve performance
- **Progress tracking**: Configurable progress callbacks for monitoring long-running searches
- **Flexible input**: Support for CSV files or Google Sheets

## Algorithm Details

The solver uses **A\*** search over an implicit state graph:

- **States**: Encode all nodes' `(infra, civilian, military)` values
- **Transitions**: Deterministic actions that modify one node at a time
- **Heuristic**: Admissible lower bound on remaining cost using best-case assumptions (max infrastructure, optimal
  conversions)
- **Cost model**: Time-based costs that account for infrastructure multipliers and factory allocation
- **Goal detection**: Terminates when target factory count is reached

The heuristic is **admissible** (never overestimates true cost) and **consistent** (satisfies triangle inequality),
ensuring A\* finds optimal solutions while exploring fewer states than Dijkstra.

For detailed algorithm documentation, see:

- `docs/DESIGN.md` - Overall architecture and problem modeling
- `docs/MODELING.md` - State space, transitions, and cost model
- `docs/DESIGN_PARALLEL.md` - Parallel construction modeling
- `docs/PRUNING.md` - Upper-bound pruning strategy
- `docs/DESIGN_PLUGGABLE_HEURISTICS.md` - Heuristic architecture
