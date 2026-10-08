"""Behavioral regressions for inventory and report validation; synthetic data only."""

from __future__ import annotations

import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import zipfile
from unittest.mock import patch

SKILL = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SKILL / "scripts"))
import project_inventory as inventory
import report_lint as lint


def report_json():
    return {
        "metadata": {"project": "fixture", "auditModes": ["security"],
                     "date": "2026-10-08", "reviewer": "test", "commitHash": "fixture"},
        "executiveSummary": {"text": "No confirmed defects in the inspected fixture."},
        "scoreDashboard": {"dimensions": [], "overall": {"score": None, "grade": "Not assessed"}},
        "findingStatistics": {
            "bySeverity": [{"severity": severity, "count": 0, "confirmed": 0, "suspected": 0}
                           for severity in lint.SEVERITIES],
            "total": {"count": 0, "confirmed": 0, "suspected": 0}},
        "projectMap": {"structure": "Fixture", "keyComponents": [], "riskAreas": []},
        "coverageMatrix": [{"dimension": "security", "coverage": "Not assessed",
                            "evidenceInspected": "Fixture only", "exclusions": "No runtime"}],
        "topRisks": [], "detailedFindings": [], "dimensionSections": {"security": {}},
        "fixOrder": [], "quickWins": [],
    }


class InventoryTests(unittest.TestCase):
    def test_prunes_dependencies_and_nested_worktrees_before_descent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for relative in ["src/app.ts", "node_modules/pkg/index.js", "src-tauri/target/lib.rs",
                             ".worktree-v173/src/main.rs", "nested/.git", "nested/src/duplicate.rs"]:
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("", encoding="utf-8")
            visited = []
            real_walk = inventory.os.walk
            def observe(*args, **kwargs):
                for row in real_walk(*args, **kwargs):
                    visited.append(Path(row[0]).relative_to(root).as_posix())
                    yield row
            with patch.object(inventory.os, "walk", observe):
                paths = inventory.iter_files(root, 100)
            self.assertEqual([p.relative_to(root).as_posix() for p in paths], ["src/app.ts"])
            self.assertEqual(set(visited), {".", "src", "src-tauri"})

    def test_exact_limit_is_complete_and_over_limit_is_truncated(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "a.rs").touch()
            self.assertFalse(inventory.build_inventory(root, 1)["scan_limit_reached"])
            (root / "b.rs").touch()
            result = inventory.build_inventory(root, 1)
            self.assertTrue(result["scan_limit_reached"])
            self.assertEqual(result["files_scanned"], 1)

    def test_rejects_invalid_limit_at_cli(self):
        result = subprocess.run([sys.executable, "-B", str(SKILL / "scripts/project_inventory.py"),
                                 "--max-files", "0"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)

    def test_svelte_tauri_detected_without_storage_or_skill_llm_false_positive(self):
        root = Path("fixture")
        files = [root / name for name in ["src/routes/+page.svelte", "src-tauri/src/storage/mod.rs",
                                         "src-tauri/src/commands/capture.rs",
                                         "src-tauri/icons/android/mipmap/icon.png",
                                         "src-tauri/icons/ios/icon.png",
                                         "skills/a/prompts/ai-safety.md"]]
        surfaces = inventory.detect_surfaces(root, files, [], '"svelte" "tauri" "rusqlite" "export"')
        self.assertTrue({"frontend", "desktop", "database", "backend_api"} <= set(surfaces))
        self.assertNotIn("ai_llm", surfaces)
        self.assertNotIn("deployment", surfaces)
        self.assertNotIn("mobile", surfaces)
        real_llm = inventory.detect_surfaces(root, [root / "src/rag/retrieve.py"], [], "")
        self.assertIn("ai_llm", real_llm)
        mobile = inventory.detect_surfaces(root, [root / "android/app/src/main/AndroidManifest.xml"], [], "")
        self.assertIn("mobile", mobile)

    def test_svelte_routes_alone_do_not_establish_backend(self):
        root = Path("fixture")
        surfaces = inventory.detect_surfaces(root, [root / "src/routes/+page.svelte"], [], "svelte")
        self.assertNotIn("backend_api", surfaces)


class LintTests(unittest.TestCase):
    def run_json(self, report, modes=None):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "report.json"
            path.write_text(json.dumps(report), encoding="utf-8")
            return lint.lint_report(path, modes)

    def test_valid_zero_finding_json_and_null_score(self):
        self.assertEqual(self.run_json(report_json(), "security"), [])

    def test_json_missing_field_wrong_type_and_score_range(self):
        for mutation in [lambda r: r.pop("metadata"),
                         lambda r: r["scoreDashboard"]["overall"].update(score=11),
                         lambda r: r["scoreDashboard"]["overall"].update(score=10**1000),
                         lambda r: r["findingStatistics"]["total"].update(count=True)]:
            report = report_json()
            mutation(report)
            self.assertTrue(self.run_json(report))

    def test_markdown_and_html_zero_finding_reports(self):
        with tempfile.TemporaryDirectory() as tmp:
            markdown = "\n\n".join(f"## {title}\nInspected synthetic fixture." for title in lint.REQUIRED_TEXT_SECTIONS)
            markdown += "\n\n## Security\nNo findings.\n"
            html = "<!doctype html><html><body>" + "".join(
                f'<section id="{key}">Inspected fixture.</section>'
                for key in (*lint.REQUIRED_HTML_IDS, "security")) + "</body></html>"
            for suffix, content in [("md", markdown), ("html", html)]:
                path = Path(tmp) / f"report.{suffix}"
                path.write_text(content, encoding="utf-8")
                self.assertEqual(lint.lint_report(path, "security"), [])

    def test_json_missing_dimension_and_inconsistent_statistics(self):
        report = report_json()
        report["dimensionSections"] = {}
        report["findingStatistics"]["total"]["count"] = 1
        issues = self.run_json(report)
        self.assertTrue(any("dimension section" in issue for issue in issues))
        self.assertTrue(any("statistics" in issue for issue in issues))

    def test_json_invalid_reference_and_duplicate_findings(self):
        report = report_json()
        finding = {key: "fixture" for key in ["id", "title", "category", "affectedArea", "failureScenario",
                                             "userVisibleImpact", "minimalFix", "regressionTestSuggestion", "estimatedEffort"]}
        finding.update(severity="Low", confidence="High", status="Confirmed",
                       evidence={"file": "src/app.ts", "functionOrModule": "save", "relevantBehavior": "fixture"})
        report["detailedFindings"] = [finding, finding.copy()]
        report["fixOrder"] = [{"priority": 1, "findingId": "missing", "reason": "fixture"}]
        issues = self.run_json(report)
        self.assertIn("Duplicate finding ids", issues)
        self.assertIn("fixOrder: unknown finding reference", issues)

    def test_secrets_are_detected_without_echoing_values(self):
        synthetic = "SYNTHETIC_TEST_TOKEN_123456789"
        for text in [f'password = "{synthetic}"', json.dumps({"api_key": synthetic}),
                     "-----BEGIN PRIVATE KEY-----"]:
            issues = []
            lint.lint_secrets(text, issues)
            self.assertTrue(issues)
            self.assertNotIn(synthetic, str(issues))
            self.assertNotIn(synthetic[:8], str(issues))
        report = report_json()
        report["executiveSummary"]["text"] = f"api_key={synthetic}"
        issues = self.run_json(report)
        self.assertTrue(issues)
        self.assertNotIn(synthetic, str(issues))

    def test_redacted_secrets_are_allowed(self):
        issues = []
        lint.lint_secrets('password = "<redacted>"', issues)
        self.assertEqual(issues, [])

    def test_concurrency_and_incremental_modes(self):
        issues = []
        self.assertIn("concurrency", lint.expand_modes("full", issues))
        self.assertIn("concurrency", lint.expand_modes("incremental", issues))
        self.assertEqual(lint.expand_modes("incremental,security", issues), ("security",))
        self.assertEqual(issues, [])
        lint.lint_mode_sections("## Concurrency", False, "concurrency", issues)
        self.assertEqual(issues, [])

    def test_missing_selected_html_section(self):
        issues = []
        lint.lint_mode_sections('<h3 id="security">Security</h3>', True, "concurrency", issues)
        self.assertTrue(issues)

    def test_markdown_case_insensitive_severity_does_not_crash(self):
        issues = []
        lint.lint_markdown_stats("| High | 1 |\n- Severity: high\n", issues)
        self.assertEqual(issues, [])

    def test_later_sections_cannot_supply_missing_finding_fields(self):
        text = "### Finding: short\n- Severity: Low\n## Next section\n- Minimal fix: unrelated\n"
        chunks = lint.finding_chunks(text)
        self.assertNotIn("unrelated", chunks[0])
        issues = []
        lint.lint_markdown_findings(text, issues)
        self.assertIn("Finding #1 missing field: Minimal fix", issues)

    def test_cli_preserves_io_failure_code_and_hides_invalid_json(self):
        with tempfile.TemporaryDirectory() as tmp:
            bad = Path(tmp) / "bad.json"
            bad.write_text('{"password": "SYNTHETIC_TEST_TOKEN_123456789"', encoding="utf-8")
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                result = lint.main([str(Path(tmp) / "missing.md"), str(bad)])
            self.assertEqual(result, 2)
            self.assertNotIn("SYNTHETIC_TEST_TOKEN", out.getvalue() + err.getvalue())


class PackageTests(unittest.TestCase):
    def test_archive_contains_resources_and_excludes_cache(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "skill.zip"
            result = subprocess.run([sys.executable, "-X", "utf8", "-B",
                                     str(SKILL / "scripts/package_skill.py"), "--output", str(output)],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            with zipfile.ZipFile(output) as archive:
                names = archive.namelist()
                prefix = "fuck-my-shit-mountain/"
                for resource in ["SKILL.md", "references/clipboard.md", "scripts/schema_check.py",
                                 "templates/audit-report.json", "prompts/concurrency-audit.md"]:
                    self.assertIn(prefix + resource, names)
                self.assertFalse(any("__pycache__" in name or name.endswith(".pyc") for name in names))
                self.assertFalse(any(name.startswith(prefix + "result/") for name in names))

    def test_archive_excludes_all_audit_results_but_keeps_templates(self):
        with tempfile.TemporaryDirectory() as tmp:
            skill = Path(tmp) / "fixture-skill"
            for name in ["SKILL.md", "templates/audit-report.md", "result/report.md",
                         "result/report.html", "result/report.json", "result/run/evidence.txt"]:
                path = skill / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("synthetic fixture", encoding="utf-8")
            output = Path(tmp) / "skill.zip"
            result = subprocess.run([sys.executable, "-X", "utf8", "-B",
                                     str(SKILL / "scripts/package_skill.py"),
                                     "--skill-dir", str(skill), "--output", str(output)],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            with zipfile.ZipFile(output) as archive:
                self.assertEqual(set(archive.namelist()), {
                    "fixture-skill/SKILL.md", "fixture-skill/templates/audit-report.md"})


if __name__ == "__main__":
    unittest.main()
