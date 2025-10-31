from typing import List

import click
import pandas as pd
from .sheets import load_nodes_from_gsheet, Node
try:
    import hoi4_mdp_core, os, glob, stat
    pkg_dir = os.path.dirname(hoi4_mdp_core.__file__)
    so = glob.glob(os.path.join(pkg_dir, '*.so'))[0]
    print(f'Loaded from: {so}')
    print(f'Modified: {os.stat(so).st_mtime}')
    import time
    print(f'Time: {time.ctime(os.stat(so).st_mtime)}')

except Exception as e:
    raise RuntimeError("Rust core (hoi4_mdp_core) is required. Ensure it is built and importable.") from e


def _safe_int(value) -> int:
    """
    Safely convert a value to int, treating empty/whitespace/NaN as 0.
    """
    if pd.isna(value):
        return 0
    s = str(value).strip()
    if not s or s == '':
        return 0
    try:
        return int(float(s))  # Handle "1.0" -> 1
    except (ValueError, TypeError):
        return 0


def load_nodes_from_csv(path: str) -> List["Node"]:
    df = pd.read_csv(path)
    # Normalize column names: lowercase, remove spaces/underscores
    def canon(s: str) -> str:
        return s.strip().lower().replace(" ", "").replace("_", "")
    inv = {canon(c): c for c in df.columns}
    def pick(*aliases: str) -> str:
        for a in aliases:
            if a in inv:
                return inv[a]
        raise ValueError(f"Missing required column; tried aliases {aliases}")

    # Map required columns using aliases
    col_name = inv.get("nodename") or pick("name", "state", "node", "province")
    col_slots = inv.get("numslots") or pick("slots", "buildingslots")
    col_infra = inv.get("numinfra") or pick("infra", "infrastructure")
    col_civ = inv.get("numcivilian") or pick("civilian", "civ", "civilianfactories")
    col_mil = inv.get("nummilitary") or pick("military", "mil", "militaryfactories")

    df = df.rename(columns={
        col_name: "nodeName",
        col_slots: "numSlots",
        col_infra: "numInfra",
        col_civ: "numCivilian",
        col_mil: "numMilitary",
    })
    # Optional columns: Docks, Refineries (subtract from numSlots)
    # Optional docks/refineries with aliasing
    docks_alias = None
    for a in ("docks", "dockyards", "navaldockyards"):
        if a in inv:
            docks_alias = inv[a]
            break
    ref_alias = None
    for a in ("refineries", "syntheticrefineries", "refinery"):
        if a in inv:
            ref_alias = inv[a]
            break
    if docks_alias is None:
        df["Docks"] = 0
    else:
        df = df.rename(columns={docks_alias: "Docks"})
    if ref_alias is None:
        df["Refineries"] = 0
    else:
        df = df.rename(columns={ref_alias: "Refineries"})
    nodes: List[Node] = []
    for _, row in df.iterrows():
        effective_slots = _safe_int(row["numSlots"]) - _safe_int(row.get("Docks", 0)) - _safe_int(row.get("Refineries", 0))
        if effective_slots < 0:
            raise ValueError(f"Effective slots negative after subtracting Docks/Refineries for node {row['nodeName']}")
        node = Node(
            name=str(row["nodeName"]),
            num_slots=effective_slots,
            num_infra=_safe_int(row["numInfra"]),
            num_civilian=_safe_int(row["numCivilian"]),
            num_military=_safe_int(row["numMilitary"]),
        )
        if node.num_infra < 0 or node.num_infra > 5:
            raise ValueError(f"numInfra out of range [0,5] for node {node.name}")
        if node.num_civilian < 0 or node.num_military < 0:
            raise ValueError(f"Negative factories on node {node.name}")
        if node.num_civilian + node.num_military > node.num_slots:
            raise ValueError(f"Capacity exceeded on node {node.name}")
        nodes.append(node)
    return nodes


@click.command()
@click.option("--input", "input_path", required=False, type=click.Path(exists=True, dir_okay=False, readable=True), help="Input nodes CSV")
@click.option("--sheet-url", "sheet_url", required=False, type=str, help="Google Sheet URL (will read the active tab via CSV export)")
@click.option("--target-type", "target_type", required=True, type=click.Choice(["military", "civilian", "factories"], case_sensitive=False), help="Type of target: military (military factories only), civilian (civilian factories only), or factories (total factories)")
@click.option("--target", "target_value", required=True, type=int, help="Target value for the selected target type")
@click.option("--moves-out", "moves_out", required=True, type=click.Path(writable=True, dir_okay=False), help="Output CSV for moves")
@click.option("--final-out", "final_out", required=True, type=click.Path(writable=True, dir_okay=False), help="Output CSV for final state")
@click.option("--no-prune", "no_prune", is_flag=True, default=False, help="Disable pruning (default: pruning enabled)")
@click.option("--verbose/--quiet", "verbose", default=True, show_default=True, help="Print periodic progress (controls progress callback)")
@click.option("--print-every", "print_every", default=10000, show_default=True, type=int, help="Print progress every N iterations (0 to disable)")
@click.option("--heuristic", "heuristic_name", default="best_infra_upper_bound", show_default=True, type=click.Choice(["best_infra_upper_bound", "standard", "djikstra", "dijkstra", "zero"], case_sensitive=False), help="Heuristic to use: best_infra_upper_bound (alias: standard) or djikstra (aliases: dijkstra, zero)")
def main(input_path: str | None, sheet_url: str | None, target_type: str, target_value: int, moves_out: str, final_out: str, no_prune: bool, verbose: bool, print_every: int, heuristic_name: str) -> None:
    if bool(input_path) == bool(sheet_url):
        raise click.UsageError("Provide exactly one of --input or --sheet-url")
    nodes = load_nodes_from_csv(input_path) if input_path else load_nodes_from_gsheet(sheet_url)  # type: ignore[arg-type]

    # Normalize target type
    target_type_lower = target_type.lower()

    # Calculate initial values
    init_total_mil = sum(n.num_military for n in nodes)
    init_total_civ = sum(n.num_civilian for n in nodes)
    init_total_factories = init_total_mil + init_total_civ

    # Validate target based on type
    if target_type_lower == "military":
        if target_value < init_total_mil:
            raise ValueError(f"Target military ({target_value}) is less than current total military ({init_total_mil})")
        max_possible = sum(n.num_slots for n in nodes)
        if target_value > max_possible:
            raise ValueError(f"Infeasible target: target={target_value} exceeds capacity={max_possible}")
        needed = max(0, target_value - init_total_mil)
        click.echo(f"Current military: {init_total_mil} | Target military: {target_value} | Additional needed: {needed} | Max possible: {max_possible}")
    elif target_type_lower == "civilian":
        if target_value < init_total_civ:
            raise ValueError(f"Target civilian ({target_value}) is less than current total civilian ({init_total_civ})")
        max_possible = sum(n.num_slots for n in nodes)
        if target_value > max_possible:
            raise ValueError(f"Infeasible target: target={target_value} exceeds capacity={max_possible}")
        needed = max(0, target_value - init_total_civ)
        click.echo(f"Current civilian: {init_total_civ} | Target civilian: {target_value} | Additional needed: {needed} | Max possible: {max_possible}")
    elif target_type_lower == "factories":
        if target_value < init_total_factories:
            raise ValueError(f"Target factories ({target_value}) is less than current total factories ({init_total_factories})")
        max_possible = sum(n.num_slots for n in nodes)
        if target_value > max_possible:
            raise ValueError(f"Infeasible target: target={target_value} exceeds capacity={max_possible}")
        needed = max(0, target_value - init_total_factories)
        click.echo(f"Current factories: {init_total_factories} | Target factories: {target_value} | Additional needed: {needed} | Max possible: {max_possible}")
    else:
        raise ValueError(f"Invalid target type: {target_type}")

    empty_slots = sum(n.num_slots - (n.num_civilian + n.num_military) for n in nodes)
    click.echo(f"Empty slots: {empty_slots}")

    rust_nodes = [(n.name, int(n.num_slots), int(n.num_infra), int(n.num_civilian), int(n.num_military)) for n in nodes]
    # Immediate banner to confirm run start before entering the solver
    # Progress formatting helpers
    def _fmt_count(n: int) -> str:
        if n >= 1_000_000_000:
            return f"{n/1_000_000_000:.2f}B"
        if n >= 1_000_000:
            return f"{n/1_000_000:.2f}M"
        if n >= 1_000:
            return f"{n/1_000:.2f}K"
        return str(n)

    def _progress_cb(snap) -> bool:
        # snap: ProgressSnapshot; fields are read-only attributes
        iters = _fmt_count(getattr(snap, "iterations"))
        cost = getattr(snap, "cost_from_start")
        heap = _fmt_count(getattr(snap, "heap_size"))
        states = _fmt_count(getattr(snap, "total_states"))
        avg_f = getattr(snap, "avg_f")
        pruned = _fmt_count(getattr(snap, "pruned"))
        ub = getattr(snap, "best_upper_bound")
        print(f"[A*] iters={iters} cost={cost:.4f} heap={heap} states={states} avg_f={avg_f:.4f} pruned={pruned} ub: best_ub={ub:.4f}", flush=True)
        return False  # never stop from CLI unless extended

    if verbose:
        print(f"[A*] invoking rust core: target_type={target_type_lower}, target={target_value}, nodes={len(rust_nodes)}, heuristic={heuristic_name}", flush=True)
    try:
        # New signature (preferred): nodes, target_type, target, *, print_every, prune, heuristic, progress_callback
        # Pass required args positionally to accommodate environments that reject target_type as a keyword
        moves, final_state_vec, total_cost = hoi4_mdp_core.solve_and_reconstruct(
            rust_nodes, target_type_lower, int(target_value),
            print_every=print_every, prune=not no_prune, heuristic=heuristic_name,
            progress_callback=_progress_cb if verbose else None,
        )
    except TypeError:
        # Legacy signature fallback: (nodes, target_military, *, verbose, print_every, re_prune)
        # Use military target; disable pruning if --no-prune; progress callback not supported in legacy.
        if target_type_lower != "military":
            raise click.UsageError("Installed core uses legacy signature (military-only). Run with --target-type military or rebuild core.")
        moves, final_state_vec, total_cost = hoi4_mdp_core.solve_and_reconstruct(
            rust_nodes, int(target_value), verbose=verbose, print_every=print_every, re_prune=not no_prune,
        )
    goal_state = tuple((int(i), int(c), int(m)) for (i, c, m) in final_state_vec)

    # Write moves CSV
    moves_df = pd.DataFrame([{"step": i + 1, "nodeName": n, "action": a} for i, (n, a) in enumerate(moves)])
    moves_df.to_csv(moves_out, index=False)

    # Write final state CSV with same schema as input
    final_rows = []
    for i, node in enumerate(nodes):
        infra, civ, mil = goal_state[i]
        final_rows.append({
            "nodeName": node.name,
            "numSlots": node.num_slots,
            "numInfra": infra,
            "numCivilian": civ,
            "numMilitary": mil,
        })
    pd.DataFrame(final_rows).to_csv(final_out, index=False)

    click.echo(f"Wrote {len(moves)} moves to {moves_out} and final state to {final_out}")


if __name__ == "__main__":
    main()
