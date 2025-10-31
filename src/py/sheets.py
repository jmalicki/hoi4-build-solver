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


# Extract sheet ID from various URL formats
_SHEET_ID_RE = re.compile(r"/spreadsheets/d/([a-zA-Z0-9-_]+)")
# Extract gid from URL (optional)
_GID_RE = re.compile(r"[#&?]gid=(\d+)")


def to_export_csv_url(sheet_url: str) -> str:
    """
    Convert a Google Sheets URL to a CSV export URL.

    Supports URLs with or without gid parameter:
    - With gid: .../edit?gid=123 -> .../export?format=csv&gid=123
    - Without gid: .../edit -> .../export?format=csv (exports first tab)
    """
    # Extract sheet ID
    sheet_id_match = _SHEET_ID_RE.search(sheet_url)
    if not sheet_id_match:
        raise ValueError(
            "Could not find sheet ID in URL. "
            "Expected format: https://docs.google.com/spreadsheets/d/SHEET_ID/..."
        )
    sheet_id = sheet_id_match.group(1)

    # Extract gid if present (from query params, hash, or #gid=...)
    gid_match = _GID_RE.search(sheet_url)
    if gid_match:
        gid = gid_match.group(1)
        return f"https://docs.google.com/spreadsheets/d/{sheet_id}/export?format=csv&gid={gid}"
    else:
        # No gid specified, export first tab
        return f"https://docs.google.com/spreadsheets/d/{sheet_id}/export?format=csv"


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
        # Skip rows with empty/invalid names
        node_name = str(row["nodeName"]).strip()
        if not node_name or node_name == '' or node_name.lower() == 'nan':
            continue

        effective_slots = _safe_int(row["numSlots"]) - _safe_int(row.get("Docks", 0)) - _safe_int(row.get("Refineries", 0))
        if effective_slots < 0:
            raise ValueError(f"Effective slots negative after subtracting Docks/Refineries for node {node_name}")

        num_infra = _safe_int(row["numInfra"])
        if num_infra < 0 or num_infra > 5:
            # Skip rows with invalid infra values
            continue

        node = Node(
            name=node_name,
            num_slots=effective_slots,
            num_infra=num_infra,
            num_civilian=_safe_int(row["numCivilian"]),
            num_military=_safe_int(row["numMilitary"]),
        )
        if node.num_civilian < 0 or node.num_military < 0:
            raise ValueError(f"Negative factories on node {node.name}")
        if node.num_civilian + node.num_military > node.num_slots:
            raise ValueError(f"Capacity exceeded on node {node.name}")
        nodes.append(node)
    return nodes
