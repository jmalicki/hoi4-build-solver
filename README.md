# HOI4 Build Solver

## Setup with uv and virtualenv

Prerequisites: Python 3.10+ and `uv` installed. Install uv from
`https://docs.astral.sh/uv/getting-started/installation/`.

Create and activate a virtualenv managed by uv:

```bash
uv venv .venv
source .venv/bin/activate
uv sync
```

This installs dependencies from `pyproject.toml` into `.venv`.

## CSV format

Input CSV must have columns:

- nodeName (str)
- numSlots (int ≥ 0)
- numInfra (int in [0,5])
- numCivilian (int ≥ 0)
- numMilitary (int ≥ 0)

Optional columns (if present, they are subtracted from numSlots before
modeling):

- Docks (int ≥ 0)
- Refineries (int ≥ 0)

Constraint per node: numMilitary + numCivilian ≤ numSlots.

## Run the solver

```bash
hoi4-mdp-solve \
  --input nodes.csv \
  --target 30 \
  --moves-out moves.csv \
  --final-out final_state.csv
```

### Flags

- `--input`: path to input CSV
- `--target`: target total military factories across all nodes
- `--moves-out`: where to write the action sequence
- `--final-out`: where to write the final node states

Alternatively, read directly from a Google Sheet (uses CSV export of the active
tab):

```bash
hoi4-mdp-solve \
  --sheet-url "https://docs.google.com/spreadsheets/d/.../edit?gid=1859149470#gid=1859149470" \
  --target 30 \
  --moves-out moves.csv \
  --final-out final_state.csv
```

Provide exactly one of `--input` or `--sheet-url`.

## Notes

- Uses A\* over an implicit state graph with an admissible, consistent
  heuristic; no full state enumeration.
- Goal condition: sum(numMilitary) == target.
