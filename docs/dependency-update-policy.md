# Dependency update policy

## Overview

stellopay-core uses **GitHub Dependabot** to keep Rust crate versions and
GitHub Actions versions current automatically.

This document describes what Dependabot does, how PRs are structured, and
what reviewers must do before merging.

---

## Ecosystems covered

| Ecosystem | Scope | Schedule |
|-----------|-------|----------|
| `cargo` | All crates in the workspace root (`/`) | Weekly, Monday 03:00 UTC |
| `github-actions` | All files under `.github/workflows/` — `ci.yml`, `contracts.yml`, `security-scan.yml` | Weekly, Monday 03:00 UTC |

Configuration lives in [`.github/dependabot.yml`](../.github/dependabot.yml).

---

## PR grouping

### Cargo

| Update type | Behaviour |
|-------------|-----------|
| Patch (`x.y.Z`) | Batched into **one PR** per week |
| Minor (`x.Y.z`) | **Individual PRs** — each crate's changelog reviewed separately |
| Major (`X.y.z`) | **Excluded from automation** — must be done manually |

Major version bumps are excluded because this is a financial contract codebase.
A major bump of `soroban-sdk` or `stellar-xdr` can change APIs in breaking
ways and must be a deliberate, reviewed decision — not an automated commit.

### GitHub Actions

All action version bumps across `ci.yml`, `contracts.yml`, and
`security-scan.yml` are batched into **one PR per week**.

---

## PR labels

Every Dependabot PR is automatically labelled:

| Label | Applied to |
|-------|-----------|
| `dependencies` | All Dependabot PRs |
| `cargo` | Rust/Cargo crate updates |
| `github-actions` | Action version updates |
| `automated-pr` | All Dependabot PRs |

---

## Review policy

> ⚠️ **Auto-merge is disabled.** Every Dependabot PR requires a human
> review and approval before it can merge. Do not enable blind auto-merge
> on a contract codebase.

**For routine patch/minor bumps:**

1. Read the release notes linked in the Dependabot PR body.
2. Check the [GitHub Advisory Database](https://github.com/advisories) for
   associated CVEs.
3. Run `cargo test --workspace` locally if the bump touches a core dependency
   (`soroban-sdk`, `stellar-xdr`, `stellar-strkey`, etc.).
4. Approve and merge once satisfied.

**For security advisory PRs (Dependabot security alerts):**

- Treat as **P0**.
- Review within **24 hours**.
- Merge or mitigate within **72 hours**.

---

## Deferring an update

If a bump must be skipped (e.g. a minor update introduces an incompatible API
before the codebase is ready), close the Dependabot PR with a comment: