# Pre-commit and CI Setup Plan

This document outlines the plan for setting up pre-commit hooks, GitHub Actions CI, README badges, and MIT license for this repository. It is based on analysis of existing repositories under `github.com/jmalicki/`.

## Project Context

This repository is a **Python + Rust (PyO3)** hybrid project:
- **Python**: Primary interface using `uv` for dependency management
- **Rust**: Core library (`hoi4_mdp_core`) compiled as a PyO3 extension module
- **Structure**: Python CLI wrapper around Rust core for MDP solver functionality

## 1. Pre-commit Hooks Configuration

### Recommended `.pre-commit-config.yaml`

Based on patterns observed in `arsync`, `econ-graph`, and `apocalypse-now-essay` repositories:

```yaml
# Pre-commit configuration for Python + Rust hybrid project
# Install: pipx install pre-commit (or brew install pre-commit)
# Setup: pre-commit install && pre-commit install --hook-type commit-msg

repos:
  # General file quality checks
  - repo: https://github.com/pre-commit/pre-commit-hooks
    rev: v4.5.0
    hooks:
      - id: trailing-whitespace
        exclude: '\.md$'  # Markdown may need trailing whitespace in some cases
      - id: end-of-file-fixer
        exclude: '\.(md|txt|mp4|aiff|wav)$'  # Some files shouldn't have newlines
      - id: check-yaml
        args: ['--unsafe']  # Allow custom tags if needed
      - id: check-json
        exclude: 'package.*\.json$'  # package.json may have comments
      - id: check-toml
      - id: check-merge-conflict
      - id: check-added-large-files
        args: ['--maxkb=5000']  # 5MB limit
      - id: mixed-line-ending
        args: ['--fix=lf']  # Enforce LF line endings
      - id: check-case-conflict
      - id: check-docstring-first  # Python-specific
      - id: detect-private-key

  # Python formatting and linting
  - repo: https://github.com/psf/black
    rev: 24.1.0
    hooks:
      - id: black
        language_version: python3
        args: ['--line-length=100']
        files: '^src/py/.*\.py$'

  # Python linting (optional - consider ruff as alternative)
  - repo: https://github.com/pycqa/flake8
    rev: 7.0.0
    hooks:
      - id: flake8
        args: ['--max-line-length=100', '--extend-ignore=E203,W503']
        files: '^src/py/.*\.py$'

  # Rust formatting
  - repo: https://github.com/pre-commit/mirrors-rustfmt
    rev: v1.5.1
    hooks:
      - id: rustfmt
        args: [--edition, "2021", --all]
        files: '^src/hoi4_mdp_core/.*\.rs$|^.*/Cargo\.toml$'

  # Rust linting (clippy)
  - repo: https://github.com/pre-commit/mirrors-clippy
    rev: v0.1.67
    hooks:
      - id: clippy
        args: [
          --workspace,
          --all-targets,
          --all-features,
          --no-deps,
          --,
          -D, warnings,
          -D, clippy::missing_docs_in_private_items,
          -D, clippy::missing_errors_doc,
          -D, clippy::missing_panics_doc,
          -D, clippy::must_use_candidate,
        ]
        files: '^src/hoi4_mdp_core/.*\.rs$'
        pass_filenames: false

  # Comprehensive Rust quality checks (CI-only)
  # NOTE: Do NOT run `cargo check`/`cargo test` in pre-commit; this belongs in CI for speed and stability.
  # See the CI "test" job below for how to run workspace checks and tests in GitHub Actions.

  # Security checks
  - repo: https://github.com/Yelp/detect-secrets
    rev: v1.4.0
    hooks:
      - id: detect-secrets
        args: ['--baseline', '.secrets.baseline']
        exclude: 'uv\.lock$|Cargo\.lock$'

  # Markdown linting
  - repo: https://github.com/igorshubovych/markdownlint-cli
    rev: v0.38.0
    hooks:
      - id: markdownlint
        args: ['--config', '.markdownlint.json']
        exclude: '^target/|^\.venv/'

  # Spell checking
  - repo: https://github.com/codespell-project/codespell
    rev: v2.3.0
    hooks:
      - id: codespell
        args: [
          '-L', 'crate,teh,nd',  # Extend ignore list as needed
          '--skip', '*.lock,*.toml,target/,Cargo.lock,uv.lock',
        ]

  # Shell script linting (if any shell scripts exist)
  - repo: https://github.com/jumanjihouse/pre-commit-hooks
    rev: 3.0.0
    hooks:
      - id: shellcheck
      - id: shfmt
        args: ['-w', '-s', '-i', '2']

  # Commit message linting (Conventional Commits)
  - repo: https://github.com/lint-commits-gitlint/commitlint
    rev: v19.0.3
    hooks:
      - id: commitlint
        stages: [commit-msg]
        args: ['--config', '.commitlintrc.yml']

# Global configuration
default_install_hook_types: [pre-commit, pre-push]
fail_fast: false  # Continue running hooks even if one fails
minimum_pre_commit_version: "3.0.0"
```

### Required Configuration Files

1. **`.commitlintrc.yml`** (for Conventional Commits):
```yaml
extends:
  - '@commitlint/config-conventional'
```

Optional (only if you prefer longer titles):
```yaml
rules:
  header-max-length: [2, always, 100]
```

2. **`.markdownlint.json`** (optional - if you want custom markdownlint rules):
```json
{
  "default": true,
  "MD013": false,
  "MD041": false,
  "MD033": false,
  "MD040": false
}
Rationale for temporary rule relaxations:
- MD013 (line length): Documentation lines can exceed 80/100 chars for readability of URLs and tables.
- MD041 (first line should be h1): Some docs start with badges or front matter before the title.
- MD033 (inline HTML): Occasionally needed for fine-grained formatting not expressible in pure Markdown.
- MD040 (fenced code blocks should have a language): Some generic or mixed-language blocks are intentional.

Keep the config as strict as feasible. Remove any disable above if the docs can comply without compromising clarity.
```

3. **`.secrets.baseline`** (for detect-secrets - generate with `detect-secrets scan --baseline .secrets.baseline`)

### Installation Instructions

Document in `CONTRIBUTING.md` or `README.md`:

```bash
# Install pre-commit
pipx install pre-commit
# OR: brew install pre-commit

# Install hooks
pre-commit install
pre-commit install --hook-type commit-msg

# Run on all files (one-time setup)
pre-commit run --all-files
```

## 2. GitHub Actions CI Workflows

### Recommended Workflow Structure

Based on patterns from `arsync`, `github-pr-automation-mcp`, and `diesel` repositories:

#### 2.1 Main CI Workflow (`.github/workflows/ci.yml`)

```yaml
name: CI

on:
  push:
    branches: ['main']
  pull_request:
    branches: ['main']
  merge_group:
    types: [checks_requested]

permissions:
  contents: read
  security-events: write
  checks: write
  actions: read

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  # Code Quality Checks (fast feedback)
  code-quality:
    name: Code Quality
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Setup Python
        uses: actions/setup-python@v5
        with:
          python-version: '3.10'
          cache: 'pip'

      - name: Setup uv
        uses: astral-sh/setup-uv@v4
        with:
          version: "latest"

      - name: Install uv dependencies
        run: uv sync --dev

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Cache Rust dependencies
        uses: Swatinem/rust-cache@v2
        with:
          key: cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Check Python formatting (black)
        run: uv run black --check --line-length 100 src/py

      - name: Check Python linting (flake8)
        run: uv run flake8 --max-line-length=100 --extend-ignore=E203,W503 src/py
        continue-on-error: true  # Optional - can make required later

      - name: Check Rust formatting
        run: cargo fmt --all -- --check

      - name: Run Clippy
        run: |
          cargo clippy --all-targets --all-features -- \
            -D warnings \
            -D clippy::missing_docs_in_private_items \
            -D clippy::missing_errors_doc \
            -D clippy::missing_panics_doc \
            -D clippy::must_use_candidate

  # Pre-commit hooks
  pre-commit:
    name: Pre-commit Hooks
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Setup Python
        uses: actions/setup-python@v5
        with:
          python-version: '3.10'

      - name: Install pre-commit
        run: pipx install pre-commit

      - name: Run pre-commit hooks
        uses: pre-commit/action@v3.0.1

  # Dependency Checks
  dependencies:
    name: Dependency Check
    runs-on: ubuntu-latest
    needs: code-quality
    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Cache Rust dependencies
        uses: Swatinem/rust-cache@v2
        with:
          key: cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Install cargo-deny
        uses: taiki-e/install-action@v2
        with:
          tool: cargo-deny@0.17.0

      - name: Check dependencies with cargo-deny
        run: cargo deny check

      - name: Check for outdated dependencies
        uses: taiki-e/install-action@v2
        with:
          tool: cargo-outdated@0.17
        continue-on-error: true
        run: cargo outdated --depth 1 --exit-code 1

  # Tests
  test:
    name: Test (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    needs: code-quality
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest]
        python-version: ['3.10', '3.11', '3.12']
        rust: [stable]

    steps:
      - uses: actions/checkout@v4

      - name: Setup Python
        uses: actions/setup-python@v5
        with:
          python-version: ${{ matrix.python-version }}
          cache: 'pip'

      - name: Setup uv
        uses: astral-sh/setup-uv@v4
        with:
          version: "latest"

      - name: Install uv dependencies
        run: uv sync --dev

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: ${{ matrix.rust }}

      - name: Cache Rust dependencies
        uses: Swatinem/rust-cache@v2
        with:
          key: cargo-${{ matrix.os }}-${{ matrix.rust }}-${{ hashFiles('**/Cargo.lock') }}

      - name: Build Rust extension
        run: uv run maturin develop --release

      - name: Run Python tests
        run: uv run pytest
        continue-on-error: true  # Add once tests exist

      - name: Run Rust tests
        run: cargo test --all-features --lib
        working-directory: src/hoi4_mdp_core

  # Security Audit
  security:
    name: Security Audit
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Cache Rust dependencies
        uses: Swatinem/rust-cache@v2
        with:
          key: cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Install cargo-audit
        uses: taiki-e/install-action@v2
        with:
          tool: cargo-audit@0.21

      - name: Run security audit
        run: cargo audit --deny warnings
        continue-on-error: ${{ github.event_name == 'schedule' }}

  # Documentation
  docs:
    name: Documentation
    runs-on: ubuntu-latest
    needs: [code-quality, test]
    steps:
      - uses: actions/checkout@v4

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Cache Rust dependencies
        uses: Swatinem/rust-cache@v2
        with:
          key: cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Build Rust documentation
        run: cargo doc --document-private-items --no-deps --all-features
        working-directory: src/hoi4_mdp_core
        env:
          RUSTDOCFLAGS: -D warnings

      - name: Upload docs artifact
        uses: actions/upload-artifact@v4
        with:
          name: rust-docs
          path: src/hoi4_mdp_core/target/doc
          retention-days: 30
```

#### 2.2 CodeQL Security Analysis (`.github/workflows/codeql.yml`)

```yaml
name: CodeQL Security Analysis

on:
  workflow_call:
  push:
    branches: ['main']
  pull_request:
    branches: ['main']
  schedule:
    - cron: '0 3 * * 0'  # Weekly on Sundays at 3 AM UTC

jobs:
  codeql-analysis:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      security-events: write

    strategy:
      fail-fast: false
      matrix:
        language: ['python', 'rust']

    steps:
      - name: Checkout code
        uses: actions/checkout@v4
        with:
          fetch-depth: 0  # Full history for CodeQL

      - name: Setup Python
        uses: actions/setup-python@v5
        with:
          python-version: '3.10'
        if: matrix.language == 'python'

      - name: Initialize CodeQL
        uses: github/codeql-action/init@v3
        with:
          languages: ${{ matrix.language }}

      - name: Autobuild
        uses: github/codeql-action/autobuild@v3

      - name: Perform CodeQL Analysis
        uses: github/codeql-action/analyze@v3
```

#### 2.3 Optional: Release Workflow

For automated releases based on Conventional Commits (optional):

```yaml
name: Release

on:
  push:
    branches: ['main']

jobs:
  release-please:
    runs-on: ubuntu-latest
    permissions:
      contents: write
      pull-requests: write
    steps:
      - uses: google-github-actions/release-please-action@v4
        with:
          release-type: python
          package-name: hoi4-mdp-solver
```

## 3. README Badges

Add to the top of `README.md`:

```markdown
# HOI4 Build Solver

[![CI](https://github.com/jmalicki/hoi4-buildsolve/actions/workflows/ci.yml/badge.svg)](https://github.com/jmalicki/hoi4-buildsolve/actions/workflows/ci.yml)
[![pre-commit.ci status](https://results.pre-commit.ci/badge/github/jmalicki/hoi4-buildsolve/main.svg)](https://results.pre-commit.ci/latest/github/jmalicki/hoi4-buildsolve/main)
[![Conventional Commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196.svg)](https://conventionalcommits.org)

[![CodeQL](https://github.com/jmalicki/hoi4-buildsolve/actions/workflows/codeql.yml/badge.svg)](https://github.com/jmalicki/hoi4-buildsolve/actions/workflows/codeql.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

[![Python 3.10+](https://img.shields.io/badge/python-3.10+-blue.svg)](https://www.python.org/downloads/)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org/)

---

[Rest of README content...]
```

**Note**: Replace `jmalicki/hoi4-buildsolve` with the actual repository path.

## 4. MIT License

Create `LICENSE` file in repository root:

```
MIT License

Copyright (c) 2025 Joseph Malicki

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

**Update `pyproject.toml`** to reference the license:

```toml
[project]
license = { text = "MIT" }
```

**Update `Cargo.toml`** (if publishing to crates.io):

```toml
[package]
license = "MIT"
```

## 5. Additional Considerations

### 5.1 Pre-commit.ci Integration

Enable [pre-commit.ci](https://pre-commit.ci) in repository settings for:
- Automatic updates of pre-commit hook versions
- Automatic fixes on PRs (with approval)
- Faster CI runs (cached hook environments)

### 5.2 Dependency Management

- **Python**: Already using `uv` (excellent choice)
- **Rust**: Consider adding `deny.toml` for `cargo-deny` configuration:
  ```toml
  [bans]
  deny = ["RUSTSEC-*"]

  [licenses]
  allow = ["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause"]
  ```

### 5.3 Test Coverage (Optional)

Consider adding codecov or similar for test coverage tracking:

```yaml
# Add to ci.yml
coverage:
  name: Code Coverage
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - name: Setup Python
      uses: actions/setup-python@v5
      with:
        python-version: '3.10'
    - name: Install uv
      uses: astral-sh/setup-uv@v4
    - name: Install coverage tools
      run: uv pip install pytest-cov
    - name: Run tests with coverage
      run: uv run pytest --cov --cov-report=xml
    - name: Upload to Codecov
      uses: codecov/codecov-action@v4
      with:
        files: ./coverage.xml
        flags: unittests
        name: codecov-umbrella
```

### 5.4 Branch Protection Rules

Configure in GitHub repository settings:
- Require status checks to pass before merging
- Require branches to be up to date before merging
- Include: `code-quality`, `pre-commit`, `test`, `security`, `CodeQL`

## 6. Implementation Order

1. **Create MIT LICENSE file** (immediate)
2. **Add `.pre-commit-config.yaml`** (with minimal hooks first, expand later)
3. **Create `.commitlintrc.yml`** for Conventional Commits
4. **Set up GitHub Actions workflows** (start with `ci.yml`, add others incrementally)
5. **Update README.md with badges** (after workflows are running)
6. **Enable pre-commit.ci** (optional but recommended)
7. **Run `pre-commit run --all-files`** to fix existing issues
8. **Iterate**: Add more hooks/workflows as needed

## 7. Key Patterns from jmalicki Repositories

### Observed Best Practices

1. **Rust Projects** (`arsync`, `diesel`, `compio-sync`):
   - Extensive use of `cargo clippy` with `-D warnings`
   - Cargo caching with `Swatinem/rust-cache@v2`
   - Matrix builds for multiple OS/Rust versions
   - Separate jobs for fmt/clippy, tests, dependencies, security
   - Use of `cargo-nextest` for faster test execution
   - `cargo-deny` for dependency management

2. **Python Projects** (`apocalypse-now-essay`):
   - Black for formatting
   - Flake8 for linting (though ruff is gaining popularity)
   - Pre-commit hooks for consistency

3. **Hybrid Projects** (`econ-graph`):
   - Separate hooks for frontend (TypeScript/ESLint) and backend (Rust)
   - Custom local hooks for workspace-specific checks
   - Integration of npm/rust security audits

4. **Documentation Projects** (`jmalicki.github.io`):
   - Markdown linting with markdownlint-cli
   - Vale for prose linting (for documentation-heavy projects)

### Common Workflow Structure

- **Early Fast Feedback**: Code quality checks (fmt, lint) run first
- **Parallel Execution**: Tests, security, dependencies run in parallel after quality checks
- **Documentation**: Built last, depends on successful tests
- **Concurrency**: Use `cancel-in-progress: true` to avoid wasted CI minutes

## 8. References

- [Pre-commit hooks](https://pre-commit.com/)
- [GitHub Actions](https://docs.github.com/en/actions)
- [Conventional Commits](https://www.conventionalcommits.org/)
- [Rust CI best practices](https://github.com/taiki-e/setup-rust-toolchain)
- [pre-commit.ci](https://pre-commit.ci/)

---

**Note**: This document is a planning document and will not be committed to the repository as per user requirements. It serves as a reference for implementing the CI/CD setup.

