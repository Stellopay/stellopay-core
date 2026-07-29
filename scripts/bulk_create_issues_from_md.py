#!/usr/bin/env python3
"""
Create GitHub issues from a Ghit-style markdown file (YAML frontmatter + body, separated by ++++++).

Uses `gh` CLI (not ghit). Requires: `gh auth login` and permission on the target repo.

Examples:
  python3 scripts/bulk_create_issues_from_md.py --dry-run \\
    onchain/contracts/GITHUB_ISSUES_SMART_CONTRACTS.md

  python3 scripts/bulk_create_issues_from_md.py \\
    onchain/contracts/GITHUB_ISSUES_SMART_CONTRACTS.md \\
    --repo Stellopay/stellopay-core
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path


def _parse_title(line: str) -> str:
    line = line.strip()
    m = re.match(r'title:\s*"(.*)"\s*$', line)
    if m:
        return m.group(1).replace(r"\"", '"')
    m = re.match(r"title:\s*'(.*)'\s*$", line)
    if m:
        return m.group(1)
    m = re.match(r"title:\s*(.+)\s*$", line)
    if m:
        return m.group(1).strip()
    raise ValueError(f"Bad title line: {line!r}")


def _parse_labels(block: str) -> list[str]:
    m = re.search(r"^labels:\s*(\[[^\]]*\])\s*$", block, re.MULTILINE)
    if not m:
        return []
    try:
        raw = json.loads(m.group(1).replace("'", '"'))
        if isinstance(raw, list):
            return [str(x) for x in raw]
    except json.JSONDecodeError:
        pass
    return []


def split_issues(text: str) -> list[tuple[str, str, list[str]]]:
    """Return list of (title, body, labels)."""
    chunks = re.split(r"\n\+\+\+\+\+\+\n", text)
    out: list[tuple[str, str, list[str]]] = []
    for chunk in chunks:
        if "title:" not in chunk:
            continue
        m = re.search(r"^title:\s*(.+)$", chunk, re.MULTILINE)
        if not m:
            continue
        title = _parse_title(m.group(0))
        labels = _parse_labels(chunk)
        m_body = re.search(r"^assignees:\s*[^\n]*\n---\s*\n", chunk, re.MULTILINE)
        if not m_body:
            continue
        body = chunk[m_body.end() :].strip()
        if not body:
            continue
        out.append((title, body, labels))
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description="Bulk-create GitHub issues from markdown via gh CLI.")
    ap.add_argument("markdown_file", type=Path, help="Path to GITHUB_ISSUES_*.md")
    ap.add_argument(
        "--repo",
        default="",
        help='Target "owner/repo" (default: from `gh repo view --json nameWithOwner`)',
    )
    ap.add_argument("--dry-run", action="store_true", help="Print titles only; do not call gh")
    ap.add_argument(
        "--no-labels",
        action="store_true",
        help="Do not pass --label (use if labels are not created in the repo yet)",
    )
    args = ap.parse_args()

    text = args.markdown_file.read_text(encoding="utf-8")
    issues = split_issues(text)
    if not issues:
        print("No issues parsed. Check file format (frontmatter + ++++++ separators).", file=sys.stderr)
        return 1

    repo = args.repo.strip()
    if not repo and not args.dry_run:
        r = subprocess.run(
            ["gh", "repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"],
            capture_output=True,
            text=True,
            check=False,
        )
        if r.returncode != 0:
            print(r.stderr or r.stdout, file=sys.stderr)
            return 1
        repo = r.stdout.strip()
    if not repo:
        repo = "(unknown — pass --repo)"

    print(f"Parsed {len(issues)} issue(s); target repo: {repo}\n", file=sys.stderr)

    for i, (title, body, labels) in enumerate(issues, 1):
        if args.dry_run:
            print(f"[{i}/{len(issues)}] {title}")
            if labels:
                print(f"         labels: {', '.join(labels)}")
            continue

        cmd = [
            "gh",
            "issue",
            "create",
            "--repo",
            repo,
            "--title",
            title,
            "--body",
            body,
        ]
        if not args.no_labels:
            for lb in labels:
                cmd.extend(["--label", lb])

        p = subprocess.run(cmd, text=True)
        if p.returncode != 0:
            return p.returncode
        print(f"Created [{i}/{len(issues)}]: {title}", file=sys.stderr)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
