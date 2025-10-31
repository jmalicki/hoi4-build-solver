"""Tests for the sheets module.

These tests verify the Google Sheets URL conversion and Node loading functionality.
"""

import pytest
from src.py.sheets import Node, _safe_int, to_export_csv_url


class TestSafeInt:
    """Test the _safe_int utility function for converting values to integers."""

    def test_safe_int_with_integer(self):
        """Test that _safe_int correctly handles integer inputs."""
        assert _safe_int(5) == 5
        assert _safe_int(0) == 0
        assert _safe_int(100) == 100

    def test_safe_int_with_float_string(self):
        """Test that _safe_int correctly handles float strings like '1.0'."""
        assert _safe_int("1.0") == 1
        assert _safe_int("5.0") == 5
        assert _safe_int("10.5") == 10  # Truncates to int

    def test_safe_int_with_empty_string(self):
        """Test that _safe_int returns 0 for empty strings."""
        assert _safe_int("") == 0
        assert _safe_int("   ") == 0

    def test_safe_int_with_nan(self):
        """Test that _safe_int handles NaN values."""
        import pandas as pd
        assert _safe_int(pd.NA) == 0
        assert _safe_int(float("nan")) == 0

    def test_safe_int_with_invalid_string(self):
        """Test that _safe_int returns 0 for invalid strings."""
        assert _safe_int("invalid") == 0
        assert _safe_int("abc") == 0


class TestToExportCsvUrl:
    """Test the Google Sheets URL to CSV export URL conversion."""

    def test_url_with_gid(self):
        """Test URL conversion when gid parameter is present."""
        url = "https://docs.google.com/spreadsheets/d/ABC123/edit?gid=1859149470"
        result = to_export_csv_url(url)
        assert result == "https://docs.google.com/spreadsheets/d/ABC123/export?format=csv&gid=1859149470"

    def test_url_without_gid(self):
        """Test URL conversion when gid parameter is missing."""
        url = "https://docs.google.com/spreadsheets/d/ABC123/edit"
        result = to_export_csv_url(url)
        assert result == "https://docs.google.com/spreadsheets/d/ABC123/export?format=csv"

    def test_url_with_hash_gid(self):
        """Test URL conversion when gid is in hash fragment."""
        url = "https://docs.google.com/spreadsheets/d/ABC123/edit#gid=1859149470"
        result = to_export_csv_url(url)
        assert result == "https://docs.google.com/spreadsheets/d/ABC123/export?format=csv&gid=1859149470"

    def test_url_with_invalid_format(self):
        """Test that invalid URL format raises ValueError."""
        with pytest.raises(ValueError, match="Could not find sheet ID"):
            to_export_csv_url("https://invalid-url.com")


class TestNode:
    """Test the Node dataclass."""

    def test_node_creation(self):
        """Test creating a Node with valid values."""
        node = Node(
            name="Test Node",
            num_slots=5,
            num_infra=2,
            num_civilian=1,
            num_military=2
        )
        assert node.name == "Test Node"
        assert node.num_slots == 5
        assert node.num_infra == 2
        assert node.num_civilian == 1
        assert node.num_military == 2

    def test_node_immutable(self):
        """Test that Node is immutable (frozen dataclass)."""
        node = Node(name="Test", num_slots=5, num_infra=0, num_civilian=0, num_military=0)
        with pytest.raises(Exception):  # Frozen dataclass raises error on assignment
            node.name = "Modified"  # type: ignore
