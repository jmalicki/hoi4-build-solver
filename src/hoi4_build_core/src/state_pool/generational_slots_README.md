# Generational Slots Module

## Overview

This module provides a zero-cost generational slot storage system for the state pool. It uses generation counters to detect stale indices automatically, eliminating the need for manual reference counting.

## Design Document

See `docs/DESIGN_GENERATIONAL_SLOTS.md` for the complete design specification.

## Implementation Plan

See `docs/IMPLEMENTATION_PLAN_GENERATIONAL_SLOTS.md` for the detailed implementation plan with phases, branch names, and PR instructions.

## Status

**Not yet implemented** - This is a design document for a future implementation.

## Key Concepts

### GenerationalIndex

A packed index (u32) + generation (u32) that provides automatic stale detection. When a slot is reused, its generation increments, making all previous indices for that slot invalid.

### Zero-Cost Aspects

- Generation check: Single `cmp` instruction
- Index extraction: Bit operations (compile-time optimized)
- No extra memory overhead compared to ref counting
- No runtime allocations

### Integration with Heap

The heap stores just the index portion (fits `index_bound` constraint), and we maintain a separate mapping of heap indices to full `GenerationalIndex` values for validation on pop.
