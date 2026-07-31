#!/usr/bin/env python3
"""Check per-crate line coverage against 95% threshold.

Reads a cargo-llvm-cov JSON report and exits non-zero if any contract crate
under onchain/contracts/ has less than 95% line coverage.

With --crate <name>, only that crate is evaluated. Crates with no executable
lines (e.g. pure-interface crates) pass automatically.
"""

import argparse
import json
import re
import sys


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--crate", help="Only check this specific crate")
    parser.add_argument("coverage_json", help="Path to coverage.json")
    args = parser.parse_args()

    with open(args.coverage_json) as f:
        data = json.load(f)

    crates: dict[str, dict[str, int]] = {}
    for entry in data.get("data", [{}])[0].get("files", []):
        filename = entry.get("filename", "")
        norm = filename.replace("\\", "/")
        m = re.search(r"/contracts/([^/]+)/", norm)
        if not m:
            continue
        crate = m.group(1)
        summary = entry.get("summary", {})
        lines = summary.get("lines", {})
        total = lines.get("count", 0)
        covered = lines.get("covered", 0)
        if total == 0:
            continue
        crates.setdefault(crate, {"lines": 0, "covered": 0})
        crates[crate]["lines"] += total
        crates[crate]["covered"] += covered

    target = args.crate
    if target:
        filtered = {k: v for k, v in crates.items() if k == target}
        if not filtered:
            print(f"[PASS] {target}: no executable lines (skipped)")
            sys.exit(0)
        crates = filtered

    if not crates:
        print("ERROR: No contract source files found in coverage data.")
        sys.exit(1)

    failures: list[str] = []
    for crate in sorted(crates):
        info = crates[crate]
        pct = (info["covered"] / info["lines"]) * 100.0
        mark = "PASS" if pct >= 95.0 else "FAIL"
        print(f"[{mark}] {crate}: {pct:.2f}% ({info['covered']}/{info['lines']})")
        if pct < 95.0:
            failures.append(crate)

    if failures:
        print(f"\nFAILED crates (< 95%): {', '.join(failures)}")
        sys.exit(1)

    print(f"\nAll {len(crates)} crate(s) meet the 95% line coverage threshold.")


if __name__ == "__main__":
    main()
