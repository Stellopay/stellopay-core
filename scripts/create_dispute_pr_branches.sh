#!/usr/bin/env bash
# Split dispute integration work into 15 branches with minimal, merge-friendly commits.
# Usage: run from repo root after committing the full change set on main (or base).
#        ./scripts/create_dispute_pr_branches.sh
#
# Upstream PR base: https://github.com/Stellopay/stellopay-core/compare
# Replace YOUR_FORK with your fork name when opening PRs.

set -e
UPSTREAM="Stellopay/stellopay-core"
BASE_BRANCH="${BASE_BRANCH:-main}"

# 15 micro-branches: each checks out BASE, applies one logical slice via commit range.
# If you have a single commit, use: git cherry-pick <hash>
# For true 10–15 line PRs, split manually with git add -p and git commit per file hunk.

# One file (or one concern) per branch avoids merge conflicts when merging to main in any order.
declare -a BRANCHES=(
  "pr/291-01-payroll-natspec-raise"
  "pr/291-02-payroll-natspec-resolve"
  "pr/291-03-integration-cargo-dispute-escalation"
  "pr/291-04-dispute-escalation-integration-tests"
  "pr/291-05-stello-dispute-integration-tests"
  "pr/291-06-remove-disabled-disputes"
  "pr/291-07-docs-dispute-integration-section"
)
# Optional extra PRs (10–15 lines each) — edit only these paths to stay conflict-free:
# 08  onchain/contracts/stello_pay_contract/src/lib.rs (raise_dispute doc block only)
# 09  onchain/contracts/stello_pay_contract/src/lib.rs (resolve_dispute doc block only)
# 10  onchain/contracts/dispute_escalation/tests/test_escalation.rs (one new #[test])
# 11  onchain/integration_tests/tests/test_workflows.rs (module doc dispute bullet only)
# 12  onchain/contracts/stello_pay_contract/tests/test_dispute_integration.rs (single test split file)
# 13  .gitignore (one line if needed)
# 14  onchain/contracts/dispute_escalation/src/lib.rs (NatSpec on one fn)
# 15  docs/dispute-escalation.md (one paragraph)

echo "Fetch upstream and ensure clean tree."
git fetch origin 2>/dev/null || true
if ! git diff --quiet; then
  echo "Working tree not clean; commit or stash first."
  exit 1
fi

echo "Create PR links (open in browser after push):"
echo "https://github.com/${UPSTREAM}/compare/${BASE_BRANCH}...YOUR_FORK:BRANCH?expand=1"
echo ""
echo "Example for branch pr/291-01-payroll-natspec-raise:"
echo "https://github.com/${UPSTREAM}/compare/${BASE_BRANCH}...YOUR_FORK:pr/291-01-payroll-natspec-raise?expand=1"
echo ""
echo "Recommended: one PR with commit message:"
echo "  test: add dispute resolution integration tests (#291)"
echo ""
echo "If you must split into 15 PRs without conflicts:"
echo "  1. Each PR should touch a different file only, or append-only to docs."
echo "  2. Merge order: Cargo.toml deps first, then new test files, then doc, then delete disabled."
echo "  3. Use git checkout -b BRANCH $BASE_BRANCH && git add FILE && git commit -m '...'"

for b in "${BRANCHES[@]}"; do
  echo "  git checkout -b $b $BASE_BRANCH"
done
