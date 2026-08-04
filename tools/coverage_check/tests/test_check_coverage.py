#!/usr/bin/env python3
"""Unit tests for ``tools/coverage_check/check_coverage.py``.

Run from anywhere:

    python3 -m unittest discover -s tools/coverage_check/tests -v
"""

from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

# Allow importing the checker without installing it.
sys.path.insert(
    0, os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)

import check_coverage as gate  # noqa: E402


def make_report(files: dict[str, tuple[int, int]]) -> dict:
    """Build a minimal llvm-cov JSON export from ``{filename: (covered, count)}``."""
    data_files = []
    for filename, (covered, count) in files.items():
        data_files.append(
            {
                "filename": filename,
                "segments": [],
                "expansions": [],
                "summary": {
                    "lines": {
                        "count": count,
                        "covered": covered,
                        "percent": (covered * 100.0 / count) if count else 0.0,
                    }
                },
            }
        )
    return {
        "version": "2.0.1",
        "type": "llvm.coverage.json.export",
        "data": [{"files": data_files, "totals": {}}],
    }


class WorkspaceFixture:
    """Temporary workspace with a fake ``contracts/`` tree."""

    def __init__(self, crates: dict[str, str]):
        """``crates`` maps package name -> directory name."""
        self._tmp = tempfile.TemporaryDirectory()
        root = Path(self._tmp.name)
        contracts = root / "contracts"
        contracts.mkdir()
        for package, directory in crates.items():
            manifest = contracts / directory / "Cargo.toml"
            manifest.parent.mkdir()
            manifest.write_text(
                f'[package]\nname = "{package}"\nversion = "0.0.0"\n',
                encoding="utf-8",
            )
        self.root = root

    def cleanup(self):
        self._tmp.cleanup()

    def __enter__(self):
        return self

    def __exit__(self, *_exc):
        self.cleanup()


class DiscoverCratesTests(unittest.TestCase):
    def test_discovers_package_and_directory_names(self):
        with WorkspaceFixture({"rbac": "rbac", "nft-payroll-badge": "nft_payroll_badge"}) as ws:
            crates = gate.discover_contract_crates(str(ws.root))
        self.assertEqual(
            crates,
            [
                {"package": "nft-payroll-badge", "dir": "nft_payroll_badge"},
                {"package": "rbac", "dir": "rbac"},
            ],
        )

    def test_results_sorted_by_directory(self):
        with WorkspaceFixture({"b": "b", "a": "a"}) as ws:
            crates = gate.discover_contract_crates(str(ws.root))
        self.assertEqual([c["package"] for c in crates], ["a", "b"])

    def test_missing_contracts_dir_raises(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(gate.CoverageGateError):
                gate.discover_contract_crates(str(Path(tmp) / "nope"))

    def test_manifest_without_name_raises(self):
        with tempfile.TemporaryDirectory() as tmp:
            manifest = Path(tmp) / "contracts" / "x" / "Cargo.toml"
            manifest.parent.mkdir(parents=True)
            manifest.write_text("[package]\nversion = \"0.0.0\"\n", encoding="utf-8")
            with self.assertRaises(gate.CoverageGateError):
                gate.discover_contract_crates(tmp)

    def test_empty_contracts_dir_raises(self):
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "contracts").mkdir()
            with self.assertRaises(gate.CoverageGateError):
                gate.discover_contract_crates(tmp)


class LoadReportTests(unittest.TestCase):
    def test_load_single_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "r.json"
            path.write_text(json.dumps(make_report({"contracts/rbac/src/lib.rs": (90, 100)})), encoding="utf-8")
            reports = gate.load_reports(str(path))
        self.assertEqual(len(reports), 1)
        self.assertEqual(reports[0]["data"][0]["files"][0]["filename"], "contracts/rbac/src/lib.rs")

    def test_load_directory_glob(self):
        with tempfile.TemporaryDirectory() as tmp:
            for name in ("a.json", "b.json", "ignore.txt"):
                (Path(tmp) / name).write_text(json.dumps(make_report({})), encoding="utf-8")
            reports = gate.load_reports(tmp)
        self.assertEqual([p.suffix for p in []], [])
        self.assertEqual(len(reports), 2)

    def test_missing_path_raises(self):
        with self.assertRaises(gate.CoverageGateError):
            gate.load_reports("/does/not/exist.json")

    def test_malformed_json_raises(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.json"
            path.write_text("{not json", encoding="utf-8")
            with self.assertRaises(gate.CoverageGateError):
                gate.load_reports(str(path))

    def test_empty_directory_raises(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(gate.CoverageGateError):
                gate.load_reports(tmp)


class AggregateTests(unittest.TestCase):
    def _crates(self):
        return [
            {"package": "rbac", "dir": "rbac"},
            {"package": "rbac-interface", "dir": "rbac-interface"},
        ]

    def test_groups_files_by_crate_and_sums(self):
        report = make_report(
            {
                "contracts/rbac/src/lib.rs": (90, 100),
                "contracts/rbac/src/storage.rs": (10, 20),
                "contracts/rbac-interface/src/lib.rs": (40, 40),
                "contracts/other/src/ignored.rs": (5, 10),
            }
        )
        result = gate.aggregate([report], self._crates())
        self.assertEqual(result["rbac"]["total"], 120)
        self.assertEqual(result["rbac"]["covered"], 100)
        self.assertEqual(result["rbac-interface"]["total"], 40)
        self.assertEqual(result["rbac-interface"]["covered"], 40)

    def test_absolute_and_windows_paths(self):
        report = make_report(
            {
                "C:\\work\\onchain\\contracts\\rbac\\src\\lib.rs": (10, 20),
                "/home/runner/onchain/contracts/rbac-interface/src/lib.rs": (1, 2),
            }
        )
        result = gate.aggregate([report], self._crates())
        self.assertEqual(result["rbac"]["total"], 20)
        self.assertEqual(result["rbac-interface"]["total"], 2)

    def test_hyphen_vs_underscore_dirs_do_not_collide(self):
        report = make_report(
            {
                "contracts/nft_payroll_badge/src/lib.rs": (50, 50),
                "contracts/milestone-interface/src/lib.rs": (50, 50),
            }
        )
        crates = [
            {"package": "nft-payroll-badge", "dir": "nft_payroll_badge"},
            {"package": "milestone-interface", "dir": "milestone-interface"},
        ]
        result = gate.aggregate([report], crates)
        self.assertEqual(result["nft-payroll-badge"]["total"], 50)
        self.assertEqual(result["milestone-interface"]["total"], 50)

    def test_duplicate_file_keeps_largest_total(self):
        report = make_report(
            {
                "contracts/rbac/src/lib.rs": (80, 100),
                "contracts/rbac/src/lib.rs": (90, 120),
            }
        )
        result = gate.aggregate([report], self._crates())
        self.assertEqual(result["rbac"]["total"], 120)
        self.assertEqual(result["rbac"]["covered"], 90)

    def test_duplicate_across_reports_keeps_largest_total(self):
        r1 = make_report({"contracts/rbac/src/lib.rs": (80, 100)})
        r2 = make_report({"contracts/rbac/src/lib.rs": (90, 120)})
        result = gate.aggregate([r1, r2], self._crates())
        self.assertEqual(result["rbac"]["total"], 120)
        self.assertEqual(result["rbac"]["covered"], 90)

    def test_missing_summary_lines_treated_as_zero(self):
        report = {"data": [{"files": [{"filename": "contracts/rbac/src/lib.rs"}]}]}
        result = gate.aggregate([report], self._crates())
        self.assertEqual(result["rbac"]["total"], 0)
        self.assertEqual(result["rbac"]["covered"], 0)

    def test_empty_reports(self):
        result = gate.aggregate([make_report({})], self._crates())
        self.assertEqual(result["rbac"]["total"], 0)
        self.assertEqual(result["rbac-interface"]["total"], 0)


class EvaluateTests(unittest.TestCase):
    def _crates(self):
        return [
            {"package": "rbac", "dir": "rbac"},
            {"package": "rbac-interface", "dir": "rbac-interface"},
        ]

    def _aggregated(self):
        report = make_report(
            {
                "contracts/rbac/src/lib.rs": (95, 100),
                "contracts/rbac-interface/src/lib.rs": (40, 40),
            }
        )
        return gate.aggregate([report], self._crates())

    def test_pass_when_at_or_above_threshold(self):
        results = gate.evaluate(self._aggregated(), self._crates(), min_pct=95.0)
        by_pkg = {r["package"]: r for r in results}
        self.assertEqual(by_pkg["rbac"]["status"], "PASS")  # exactly 95.0
        self.assertEqual(by_pkg["rbac-interface"]["status"], "PASS")

    def test_fail_below_threshold(self):
        results = gate.evaluate(self._aggregated(), self._crates(), min_pct=96.0)
        by_pkg = {r["package"]: r for r in results}
        self.assertEqual(by_pkg["rbac"]["status"], "FAIL")
        self.assertIn("below", by_pkg["rbac"]["reason"])
        self.assertEqual(by_pkg["rbac-interface"]["status"], "PASS")

    def test_no_data_fails(self):
        per_crate = {
            "rbac": {"dir": "rbac", "present": True, "total": 0, "covered": 0, "files": {}},
            "rbac-interface": {"dir": "rbac-interface", "present": True, "total": 0, "covered": 0, "files": {}},
        }
        results = gate.evaluate(per_crate, self._crates(), min_pct=95.0)
        for result in results:
            self.assertEqual(result["status"], "FAIL")
            self.assertIn("no coverage data", result["reason"])
            self.assertIn("0 measurable lines", result["reason"])

    def test_missing_crate_from_reports_fails(self):
        report = make_report({"contracts/rbac/src/lib.rs": (95, 100)})
        per_crate = gate.aggregate([report], self._crates())
        results = gate.evaluate(per_crate, self._crates(), min_pct=95.0)
        by_pkg = {r["package"]: r for r in results}
        self.assertEqual(by_pkg["rbac"]["status"], "PASS")
        self.assertEqual(by_pkg["rbac-interface"]["status"], "FAIL")
        self.assertIn("absent from reports", by_pkg["rbac-interface"]["reason"])

    def test_only_crate_mode(self):
        per_crate = self._aggregated()
        results = gate.evaluate(per_crate, self._crates(), min_pct=99.0, only_crate="rbac")
        self.assertEqual([r["package"] for r in results], ["rbac"])
        self.assertEqual(results[0]["status"], "FAIL")


class RenderSummaryTests(unittest.TestCase):
    def test_table_contains_all_packages(self):
        crates = [{"package": "a", "dir": "a"}, {"package": "b", "dir": "b"}]
        per_crate = gate.aggregate(
            [
                make_report(
                    {
                        "contracts/a/src/lib.rs": (50, 50),
                        "contracts/b/src/lib.rs": (80, 100),
                    }
                )
            ],
            crates,
        )
        results = gate.evaluate(per_crate, crates, min_pct=95.0)
        text = gate.render_summary(results, 95.0)
        self.assertIn("| a | a | 50 | 50 | 100.00% | 95.00% | PASS |", text)
        self.assertIn("| b | b | 100 | 80 | 80.00% | 95.00% | FAIL |", text)
        self.assertIn("### b", text)
        self.assertIn("contracts/b/src/lib.rs", text)

    def test_no_data_shows_em_dash(self):
        results = [
            {
                "package": "x",
                "dir": None,
                "total": 0,
                "covered": 0,
                "pct": 0.0,
                "threshold": 95.0,
                "status": "FAIL",
                "reason": "no coverage data",
                "files": {},
            }
        ]
        text = gate.render_summary(results, 95.0)
        self.assertIn("| x | — | 0 | 0 | — | 95.00% | FAIL |", text)


class MatchCrateTests(unittest.TestCase):
    def test_relative_vs_absolute(self):
        crates = [{"package": "rbac", "dir": "rbac"}]
        self.assertEqual(gate.match_crate("contracts/rbac/src/lib.rs", crates)["package"], "rbac")
        self.assertEqual(
            gate.match_crate("/work/onchain/contracts/rbac/src/error.rs", crates)["package"], "rbac"
        )

    def test_prefix_crates_do_not_collide(self):
        crates = [
            {"package": "rbac", "dir": "rbac"},
            {"package": "rbac-interface", "dir": "rbac-interface"},
        ]
        self.assertEqual(gate.match_crate("contracts/rbac/src/lib.rs", crates)["package"], "rbac")
        self.assertEqual(
            gate.match_crate("contracts/rbac-interface/src/lib.rs", crates)["package"],
            "rbac-interface",
        )

    def test_non_src_files_ignored(self):
        crates = [{"package": "rbac", "dir": "rbac"}]
        self.assertIsNone(gate.match_crate("contracts/rbac/tests/test_rbac.rs", crates))
        self.assertIsNone(gate.match_crate("contracts/other/src/lib.rs", crates))


class MainTests(unittest.TestCase):
    def _write_report(self, directory: Path, files: dict[str, tuple[int, int]]) -> str:
        path = directory / "report.json"
        path.write_text(json.dumps(make_report(files)), encoding="utf-8")
        return str(path)

    def test_exit_zero_when_all_pass(self):
        with WorkspaceFixture({"rbac": "rbac", "a": "a"}) as ws:
            report = self._write_report(
                ws.root,
                {
                    "contracts/rbac/src/lib.rs": (95, 100),
                    "contracts/a/src/lib.rs": (50, 50),
                },
            )
            code = gate.main(
                ["--workspace", str(ws.root), "--report", report, "--min-line-pct", "95"]
            )
        self.assertEqual(code, 0)

    def test_exit_one_when_below_threshold(self):
        with WorkspaceFixture({"rbac": "rbac"}) as ws:
            report = self._write_report(ws.root, {"contracts/rbac/src/lib.rs": (80, 100)})
            code = gate.main(
                ["--workspace", str(ws.root), "--report", report, "--min-line-pct", "95"]
            )
        self.assertEqual(code, 1)

    def test_exit_one_when_missing_crate(self):
        with WorkspaceFixture({"rbac": "rbac", "missing": "missing"}) as ws:
            report = self._write_report(ws.root, {"contracts/rbac/src/lib.rs": (95, 100)})
            code = gate.main(
                ["--workspace", str(ws.root), "--report", report, "--min-line-pct", "95"]
            )
        self.assertEqual(code, 1)

    def test_crate_mode_ignores_other_crates(self):
        with WorkspaceFixture({"rbac": "rbac", "missing": "missing"}) as ws:
            report = self._write_report(ws.root, {"contracts/rbac/src/lib.rs": (95, 100)})
            code = gate.main(
                [
                    "--workspace", str(ws.root),
                    "--report", report,
                    "--min-line-pct", "95",
                    "--crate", "rbac",
                ]
            )
        self.assertEqual(code, 0)

    def test_unknown_crate_is_usage_error(self):
        with WorkspaceFixture({"rbac": "rbac"}) as ws:
            report = self._write_report(ws.root, {"contracts/rbac/src/lib.rs": (95, 100)})
            with self.assertRaises(SystemExit) as ctx:
                gate.main(
                    [
                        "--workspace", str(ws.root),
                        "--report", report,
                        "--crate", "not-a-crate",
                    ]
                )
        self.assertEqual(ctx.exception.code, 2)

    def test_invalid_threshold_is_usage_error(self):
        with WorkspaceFixture({"rbac": "rbac"}) as ws:
            report = self._write_report(ws.root, {"contracts/rbac/src/lib.rs": (95, 100)})
            with self.assertRaises(SystemExit) as ctx:
                gate.main(
                    [
                        "--workspace", str(ws.root),
                        "--report", report,
                        "--min-line-pct", "150",
                    ]
                )
        self.assertEqual(ctx.exception.code, 2)

    def test_directory_report_mode(self):
        with WorkspaceFixture({"a": "a", "b": "b"}) as ws:
            (ws.root / "reports").mkdir()
            (ws.root / "reports" / "a.json").write_text(
                json.dumps(make_report({"contracts/a/src/lib.rs": (50, 50)})), encoding="utf-8"
            )
            (ws.root / "reports" / "b.json").write_text(
                json.dumps(make_report({"contracts/b/src/lib.rs": (40, 50)})), encoding="utf-8"
            )
            code = gate.main(
                ["--workspace", str(ws.root), "--report", str(ws.root / "reports")]
            )
        self.assertEqual(code, 1)

    def test_missing_report_exits_one(self):
        with WorkspaceFixture({"a": "a"}) as ws:
            code = gate.main(
                ["--workspace", str(ws.root), "--report", str(ws.root / "nope.json")]
            )
        self.assertEqual(code, 1)

    def test_step_summary_written_when_env_set(self):
        with WorkspaceFixture({"a": "a"}) as ws:
            report = self._write_report(ws.root, {"contracts/a/src/lib.rs": (50, 50)})
            with tempfile.TemporaryDirectory() as tmp:
                summary_path = str(Path(tmp) / "summary.md")
                old = os.environ.get("GITHUB_STEP_SUMMARY")
                os.environ["GITHUB_STEP_SUMMARY"] = summary_path
                try:
                    code = gate.main(["--workspace", str(ws.root), "--report", report])
                finally:
                    if old is None:
                        os.environ.pop("GITHUB_STEP_SUMMARY", None)
                    else:
                        os.environ["GITHUB_STEP_SUMMARY"] = old
                self.assertEqual(code, 0)
                self.assertIn("Contract Coverage Gate", Path(summary_path).read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
