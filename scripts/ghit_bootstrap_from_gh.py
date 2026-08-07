#!/usr/bin/env python3
"""
Thin wrapper: runs the canonical script from ~/.local/bin/ghit_bootstrap_from_gh.py
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

_CANONICAL = Path.home() / ".local" / "bin" / "ghit_bootstrap_from_gh.py"


def main() -> None:
    if not _CANONICAL.is_file():
        print(
            f"ERROR: missing canonical script:\n  {_CANONICAL}\n"
            "Copy the full script there (see project history or your backup).",
            file=sys.stderr,
        )
        sys.exit(1)
    os.execv(sys.executable, [sys.executable, str(_CANONICAL), *sys.argv[1:]])


if __name__ == "__main__":
    main()
