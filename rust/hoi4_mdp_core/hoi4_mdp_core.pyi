"""Typed stub for the Rust extension module `hoi4_mdp_core`.

PEP 561: This package ships type information for static type checkers.
"""
from __future__ import annotations
from typing import List, Tuple

__all__ = ["solve_and_reconstruct"]

# nodes: list[(name, numSlots, numInfra, numCivilian, numMilitary)]
# returns: (moves, final_state, total_cost)
# - moves: list[(nodeName, actionLabel)]
# - final_state: list[(infra, civ, mil)]
# - total_cost: float

def solve_and_reconstruct(
    nodes: List[Tuple[str, int, int, int, int]],
    target_military: int,
    *,
    verbose: bool = ..., 
    print_every: int = ...,
    re_prune: bool = ...,
) -> Tuple[List[Tuple[str, str]], List[Tuple[int, int, int]], float]: ...
