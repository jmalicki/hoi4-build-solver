from typing import List

import click
import pandas as pd
from .sheets import load_nodes_from_gsheet, Node
try:
    import hoi4_mdp_core  # type: ignore
except Exception as e:
    raise RuntimeError("Rust core (hoi4_mdp_core) is required. Ensure it is built and importable.") from e


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
        effective_slots = int(row["numSlots"]) - int(row["Docks"]) - int(row["Refineries"])
        if effective_slots < 0:
            raise ValueError(f"Effective slots negative after subtracting Docks/Refineries for node {row['nodeName']}")
        node = Node(
            name=str(row["nodeName"]),
            num_slots=effective_slots,
            num_infra=int(row["numInfra"]),
            num_civilian=int(row["numCivilian"]),
            num_military=int(row["numMilitary"]),
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
@click.option("--target", "target_military", required=True, type=int, help="Target total military across nodes")
@click.option("--moves-out", "moves_out", required=True, type=click.Path(writable=True, dir_okay=False), help="Output CSV for moves")
@click.option("--final-out", "final_out", required=True, type=click.Path(writable=True, dir_okay=False), help="Output CSV for final state")
@click.option("--gamma", default=0.999, show_default=True, type=float, help="Discount factor")
def main(input_path: str | None, sheet_url: str | None, target_military: int, moves_out: str, final_out: str, gamma: float) -> None:
    if bool(input_path) == bool(sheet_url):
        raise click.UsageError("Provide exactly one of --input or --sheet-url")
    nodes = load_nodes_from_csv(input_path) if input_path else load_nodes_from_gsheet(sheet_url)  # type: ignore[arg-type]
    init_total_mil = sum(n.num_military for n in nodes)
    if target_military < init_total_mil:
        raise ValueError("targetMilitary is less than current total military")
    empty_slots = sum(n.num_slots - (n.num_civilian + n.num_military) for n in nodes)
    needed_mil = max(0, target_military - init_total_mil)
    total_slots = sum(n.num_slots for n in nodes)
    max_possible_mil = total_slots
    if target_military > max_possible_mil:
        raise ValueError(
            f"Infeasible target: target={target_military} exceeds capacity={max_possible_mil}"
        )
    click.echo(
        f"Empty slots: {empty_slots} | Additional military needed: {needed_mil} | Max possible military: {max_possible_mil}"
    )

    rust_nodes = [(n.name, int(n.num_slots), int(n.num_infra), int(n.num_civilian), int(n.num_military)) for n in nodes]
    # Immediate banner to confirm run start before entering the solver
    print(f"[A*] invoking rust core: target={target_military}, nodes={len(rust_nodes)}", flush=True)
    moves, final_state_vec, total_cost = hoi4_mdp_core.solve_and_reconstruct(
        rust_nodes, int(target_military), verbose=True, print_every=1,
        prune=False,
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


