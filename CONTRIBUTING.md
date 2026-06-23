# Contributing to Stellopay Core

Thank you for your interest in contributing! This guide will help you get started.

## Getting Started

1. Fork the repository.
2. Clone your fork:
   ```sh
   git clone https://github.com/your-username/stellopay-core.git
   ```
3. Follow the build and test instructions in the [README](README.md).

## Development Workflow

### Branch Naming

Use descriptive branch names with a prefix:

- `feat/` — new features
- `fix/` — bug fixes
- `refactor/` — code restructuring
- `docs/` — documentation changes
- `test/` — test additions or improvements
- `ci/` — CI/CD changes

### Commit Messages

Write clear, concise commit messages in the following format:

```
type(scope): short description

Longer description if needed.
```

Types: `feat`, `fix`, `refactor`, `docs`, `test`, `ci`, `chore`.

### Before Submitting a PR

1. Run `cargo build` and `cargo test` from the `onchain/` workspace.
2. Ensure linting passes: `cargo clippy --all-targets`.
3. Keep PRs focused — one logical change per PR.
4. Link the PR to any related issue.

## Pull Request Process

1. Update documentation if your changes introduce new behavior.
2. Add or update tests to cover your changes.
3. Ensure all CI checks pass.
4. Request a review from a maintainer.

## Code of Conduct

Please follow the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct) in all interactions.
