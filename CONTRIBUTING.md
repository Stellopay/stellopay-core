# Contributing to stellopay-core

Thank you for contributing to stellopay-core — a decentralized payroll system
built on the Stellar blockchain using Soroban.

---

## Getting started

1. Fork the repository and create a branch from `main`.
2. Follow the local environment setup in [`docs/ci.md`](docs/ci.md).
3. Make your changes, add tests, and ensure CI passes before opening a PR.

---

## Branch naming

Use a short prefix that matches the type of change:

| Prefix | Use for |
|--------|---------|
| `feat/` | New features |
| `fix/` | Bug fixes |
| `chore/` | Tooling, config, dependency updates |
| `docs/` | Documentation only |
| `devops/` | CI/CD and infrastructure |

Example: `devops/dependabot-config`, `feat/bulk-payment-v2`

---

## Pull request checklist

- [ ] Branch is up to date with `main`
- [ ] `cargo test --workspace` passes locally
- [ ] New or changed behaviour is covered by tests
- [ ] Relevant docs under `docs/` are updated
- [ ] PR description explains *what* changed and *why*

---

## Dependency updates

stellopay-core uses **GitHub Dependabot** to keep Rust crates and GitHub
Actions versions current. Dependabot opens PRs automatically every Monday.

See [`docs/dependency-update-policy.md`](docs/dependency-update-policy.md)
for the full policy, including grouping rules, review obligations, and how
to defer an update.

**Key rules at a glance:**

- Patch Cargo bumps arrive as one grouped PR per week.
- Minor Cargo bumps open individual PRs for per-crate review.
- Major Cargo bumps are **excluded from automation** — handle manually.
- All GitHub Actions bumps (`ci.yml`, `contracts.yml`, `security-scan.yml`)
  are batched into one weekly PR.
-