from __future__ import annotations

import re
from typing import List

import pandas as pd

from dataclasses import dataclass


@dataclass(frozen=True)
class Node:
    name: str
    num_slots: int
    num_infra: int
    num_civilian: int
    num_military: int


_GDRIVE_EXPORT_RE = re.compile(r"/edit\?.*?gid=(\d+).*")


def to_export_csv_url(sheet_url: str) -> str:
    # Convert .../edit?...gid=NNN to .../export?format=csv&gid=NNN
    m = _GDRIVE_EXPORT_RE.search(sheet_url)
    if not m:
        raise ValueError("Sheet URL must contain a gid parameter; open the desired tab and copy the URL.")
    gid = m.group(1)
    base = sheet_url.split("/edit", 1)[0]
    return f"{base}/export?format=csv&gid={gid}"


def load_nodes_from_gsheet(sheet_url: str) -> List[Node]:
    csv_url = to_export_csv_url(sheet_url)
    df = pd.read_csv(csv_url)
    # Normalize column names: lowercase, remove spaces/underscores
    norm = {c: c for c in df.columns}
    def canon(s: str) -> str:
        return s.strip().lower().replace(" ", "").replace("_", "")
    inv = {canon(c): c for c in df.columns}
    def pick(*aliases: str) -> str:
        for a in aliases:
            if a in inv:
                return inv[a]
        raise ValueError(f"Missing required column; tried aliases {aliases}")

    col_name = pick("nodename", "name", "state", "node", "province")
    col_slots = pick("numslots", "slots", "buildingslots")
    col_infra = pick("numinfra", "infra", "infrastructure")
    col_civ = pick("numcivilian", "civilian", "civ", "civilianfactories")
    col_mil = pick("nummilitary", "military", "mil", "militaryfactories")

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


