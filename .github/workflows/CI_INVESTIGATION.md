# CI Not Running on Pull Requests - Investigation

## Issue
CI workflow is not running on pull requests.

## Root Cause
The CI workflow is correctly configured to trigger only on the `main` branch:
```yaml
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
```

However, all current pull requests are targeting `feat/heap-growth-guards-abi-0.1.1` instead of `main`:
- PR #1: base `feat/heap-growth-guards-abi-0.1.1`
- PR #2: base `feat/heap-growth-guards-abi-0.1.1`
- PR #3: base `feat/heap-growth-guards-abi-0.1.1`

## Solution
Pull requests must target the `main` branch for CI to run. The `main` branch should be the default branch for the repository.

## Action Required
1. Change the repository's default branch from `feat/heap-growth-guards-abi-0.1.1` to `main` in GitHub repository settings
2. Update all existing PRs to target `main` instead of `feat/heap-growth-guards-abi-0.1.1`
3. Ensure future PRs target `main`
