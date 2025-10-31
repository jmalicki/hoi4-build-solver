# hoi4_mdp_core (Rust PyO3 extension)

High-performance A* solver core for the HOI4 build-planning MDP, exposed to Python via PyO3.

- State/action/cost and heuristic definitions follow `docs/MODELING.md` in the repo root.
- Python CLI and I/O live in the `src/py` package; this crate provides only the solver.

## Build and develop

Recommended via uv + maturin from repo root:

```bash
# from repository root
uv run --no-project --with maturin \
  maturin develop --release -m src/hoi4_mdp_core/Cargo.toml
```

You can also install the Rust subpackage editable through uv:

```bash
uv pip install -e src/hoi4_mdp_core --reinstall
```

Notes:
- Requires a working Rust toolchain (rustc/cargo) and Xcode CLT on macOS.
- Targets Python 3.10+ (abi3) via `pyo3` 0.27.

## Python API

```python
from hoi4_mdp_core import solve_and_reconstruct

# nodes: list of (name, numSlots, numInfra, numCivilian, numMilitary)
# Returns: (moves, final_state, total_cost)
# - moves: list[(nodeName, actionLabel)]
# - final_state: list[(infra, civ, mil)]
# - total_cost: float
moves, final_state, total_cost = solve_and_reconstruct(
    nodes,
    target_military,
    verbose=True,        # kw-only
    print_every=250,     # kw-only
)
```

Type checkers: this package ships `hoi4_mdp_core.pyi` and `py.typed`.

## Dev tips

- If progress prints do not appear, force a rebuild:
  - `cargo clean` in `src/hoi4_mdp_core`, then run the maturin command above.
- `rapidhash` is used as the hasher for `HashMap`.
- `cargo fmt` to format; `cargo clippy` for lints.
