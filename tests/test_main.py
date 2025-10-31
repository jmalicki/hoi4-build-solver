"""Tests for the main module.

These tests verify the CSV loading and validation functionality.
"""

import tempfile
from pathlib import Path

import pandas as pd
import pytest

from src.py.__main__ import _safe_int, load_nodes_from_csv


class TestSafeInt:
    """Test the _safe_int utility function from __main__ module."""

    def test_safe_int_basic(self):
        """Test basic integer conversion."""
        assert _safe_int(42) == 42
        assert _safe_int("42") == 42

    def test_safe_int_with_float_string(self):
        """Test conversion of float strings."""
        assert _safe_int("1.0") == 1
        assert _safe_int("5.5") == 5

    def test_safe_int_with_empty_values(self):
        """Test handling of empty or None values."""
        assert _safe_int("") == 0
        assert _safe_int("   ") == 0
        import pandas as pd
        assert _safe_int(pd.NA) == 0


class TestLoadNodesFromCsv:
    """Test loading nodes from CSV files."""

    def test_load_simple_csv(self):
        """Test loading a simple valid CSV file."""
        with tempfile.NamedTemporaryFile(mode='w', suffix='.csv', delete=False) as f:
            f.write("nodeName,numSlots,numInfra,numCivilian,numMilitary\n")
            f.write("Node1,5,2,1,2\n")
            f.write("Node2,3,1,0,1\n")
            csv_path = f.name

        try:
            nodes = load_nodes_from_csv(csv_path)
            assert len(nodes) == 2
            assert nodes[0].name == "Node1"
            assert nodes[0].num_slots == 5
            assert nodes[0].num_infra == 2
            assert nodes[0].num_civilian == 1
            assert nodes[0].num_military == 2
        finally:
            Path(csv_path).unlink()

    def test_load_csv_with_docks_and_refineries(self):
        """Test loading CSV with docks and refineries (subtracted from slots)."""
        with tempfile.NamedTemporaryFile(mode='w', suffix='.csv', delete=False) as f:
            f.write("nodeName,numSlots,Docks,Refineries,numInfra,numCivilian,numMilitary\n")
            f.write("Node1,10,2,1,2,1,2\n")  # effective slots = 10 - 2 - 1 = 7
            csv_path = f.name

        try:
            nodes = load_nodes_from_csv(csv_path)
            assert len(nodes) == 1
            assert nodes[0].num_slots == 7  # Should be 10 - 2 - 1
        finally:
            Path(csv_path).unlink()

    def test_load_csv_with_negative_effective_slots(self):
        """Test that negative effective slots raise ValueError."""
        with tempfile.NamedTemporaryFile(mode='w', suffix='.csv', delete=False) as f:
            f.write("nodeName,numSlots,Docks,Refineries,numInfra,numCivilian,numMilitary\n")
            f.write("Node1,5,3,3,2,1,2\n")  # effective slots = 5 - 3 - 3 = -1
            csv_path = f.name

        try:
            with pytest.raises(ValueError, match="Effective slots negative"):
                load_nodes_from_csv(csv_path)
        finally:
            Path(csv_path).unlink()

    def test_load_csv_with_invalid_infra(self):
        """Test that infra values outside [0,5] raise ValueError."""
        with tempfile.NamedTemporaryFile(mode='w', suffix='.csv', delete=False) as f:
            f.write("nodeName,numSlots,numInfra,numCivilian,numMilitary\n")
            f.write("Node1,5,6,1,2\n")  # infra = 6 is invalid
            csv_path = f.name

        try:
            with pytest.raises(ValueError, match="numInfra out of range"):
                load_nodes_from_csv(csv_path)
        finally:
            Path(csv_path).unlink()

    def test_load_csv_with_exceeded_capacity(self):
        """Test that capacity exceeded raises ValueError."""
        with tempfile.NamedTemporaryFile(mode='w', suffix='.csv', delete=False) as f:
            f.write("nodeName,numSlots,numInfra,numCivilian,numMilitary\n")
            f.write("Node1,5,2,3,3\n")  # 3 + 3 = 6 > 5 slots
            csv_path = f.name

        try:
            with pytest.raises(ValueError, match="Capacity exceeded"):
                load_nodes_from_csv(csv_path)
        finally:
            Path(csv_path).unlink()

    def test_load_csv_with_column_aliases(self):
        """Test that CSV loading works with various column name aliases."""
        with tempfile.NamedTemporaryFile(mode='w', suffix='.csv', delete=False) as f:
            f.write("Name,Slots,Infrastructure,Civilian,Military\n")
            f.write("Node1,5,2,1,2\n")
            csv_path = f.name

        try:
            nodes = load_nodes_from_csv(csv_path)
            assert len(nodes) == 1
            assert nodes[0].name == "Node1"
        finally:
            Path(csv_path).unlink()
