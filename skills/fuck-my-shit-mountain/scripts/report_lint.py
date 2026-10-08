#!/usr/bin/env python3
"""Lint generated fuck-my-shit-mountain audit reports."""

from __future__ import annotations

import argparse
import json
import re
import sys
from html import unescape
from pathlib import Path

from schema_check import validate


SEVERITIES = ("Critical", "High", "Medium", "Low", "Info")

REQUIRED_TEXT_SECTIONS = (
    "Executive Summary",
    "Project Map",
    "Coverage Matrix",
    "Finding Statistics",
    "Top Risks",
    "Detailed Findings",
    "Recommended Fix Order",
    "Quick Wins",
)

REQUIRED_HTML_IDS = (
    "summary",
    "executive-summary",
    "coverage",
    "findings",
    "fix-order",
)

FULL_SECTION_IDS = (
    "architecture",
    "security",
    "stability",
    "performance",
    "testing",
    "maintainability",
    "design",
    "release",
    "documentation",
    "configuration",
    "observability",
    "data-integrity",
    "privacy",
    "accessibility",
    "supply-chain",
    "cost",
    "ai-safety",
    "fallback",
    "testing-authenticity",
    "type-safety",
    "frontend-state",
    "backend-api",
    "dependency-weight",
    "code-consistency",
    "comment-coverage",
    "concurrency",
)

MODE_TO_SECTION_IDS = {
    "full": FULL_SECTION_IDS,
    "incremental": FULL_SECTION_IDS,
    **{section_id: (section_id,) for section_id in FULL_SECTION_IDS},
}

MARKDOWN_SECTION_PATTERNS = {
    "architecture": r"Architecture",
    "security": r"Security",
    "stability": r"Stability",
    "performance": r"Performance",
    "testing": r"Testing",
    "maintainability": r"Maintainability",
    "design": r"Design",
    "release": r"Release",
    "documentation": r"Documentation",
    "configuration": r"Configuration",
    "observability": r"Observability",
    "data-integrity": r"Data Integrity",
    "privacy": r"Privacy",
    "accessibility": r"Accessibility",
    "supply-chain": r"Supply Chain",
    "cost": r"Cost",
    "ai-safety": r"AI.*Safety|LLM.*Safety",
    "fallback": r"Fallback",
    "testing-authenticity": r"Testing Authenticity",
    "type-safety": r"Type Safety",
    "frontend-state": r"Frontend State",
    "backend-api": r"Backend API",
    "dependency-weight": r"Dependency Weight",
    "code-consistency": r"Code Consistency",
    "comment-coverage": r"Comment Coverage",
    "concurrency": r"Concurrency",
}

REQUIRED_FINDING_FIELDS = (
    "Severity",
    "Confidence",
    "Category",
    "Status",
    "Affected area",
    "Evidence",
    "Problem",
    "Why it matters",
    "Realistic failure scenario",
    "Minimal fix",
    "Better long-term fix",
    "Regression test suggestion",
    "Estimated effort",
)

PLACEHOLDER_PATTERNS = (
    re.compile(r"\[\[[A-Z0-9_ -]+\]\]"),
    re.compile(
        r"<(?:project name|date|AI model / version|full / security|N|dimension|files, commands, patterns, runtime surfaces|what was not inspected and why|short title|selected modes?)>",
        re.IGNORECASE,
    ),
)

SECRET_PATTERNS = (
    re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH |DSA |)?PRIVATE KEY-----"),
    re.compile(
        r"(?i)\b(api[_-]?key|access[_-]?token|auth[_-]?token|secret|password|passwd)\b['\"]?\s*[:=]\s*['\"]?(?!<redacted>|redacted\b)[A-Za-z0-9_./+=:-]{16,}"
    ),
)


def add_issue(issues: list[str], message: str) -> None:
    issues.append(message)


def lint_placeholders(text: str, issues: list[str]) -> None:
    for pattern in PLACEHOLDER_PATTERNS:
        match = pattern.search(text)
        if match:
            add_issue(issues, f"Unreplaced template placeholder at character {match.start()}")


def lint_secrets(text: str, issues: list[str]) -> None:
    for pattern in SECRET_PATTERNS:
        match = pattern.search(text)
        if match:
            add_issue(issues, f"Possible unredacted secret at character {match.start()} (value omitted)")


def lint_required_sections(text: str, is_html: bool, issues: list[str]) -> None:
    if is_html:
        for section_id in REQUIRED_HTML_IDS:
            if f'id="{section_id}"' not in text and f"id='{section_id}'" not in text:
                add_issue(issues, f"Missing HTML section id: {section_id}")
        return

    for section in REQUIRED_TEXT_SECTIONS:
        if re.search(rf"(?mi)^##+\s+.*{re.escape(section)}\b", text) is None:
            add_issue(issues, f"Missing report section: {section}")


def expand_modes(modes: str | None, issues: list[str]) -> tuple[str, ...]:
    if not modes:
        return ()

    section_ids: list[str] = []
    raw_modes = [mode for mode in re.split(r"[,\s]+", modes.strip().lower()) if mode]
    if "incremental" in raw_modes and any(mode not in {"incremental", "full"} for mode in raw_modes):
        raw_modes.remove("incremental")
    for raw_mode in raw_modes:
        if not raw_mode:
            continue
        mapped = MODE_TO_SECTION_IDS.get(raw_mode)
        if mapped is None:
            add_issue(issues, f"Unknown audit mode for section check: {raw_mode}")
            continue
        for section_id in mapped:
            if section_id not in section_ids:
                section_ids.append(section_id)
    return tuple(section_ids)


def lint_mode_sections(text: str, is_html: bool, modes: str | None, issues: list[str]) -> None:
    section_ids = expand_modes(modes, issues)
    if not section_ids:
        return

    for section_id in section_ids:
        if is_html:
            if f'id="{section_id}"' not in text and f"id='{section_id}'" not in text:
                add_issue(issues, f"Missing selected dimension HTML section id: {section_id}")
            continue

        pattern = MARKDOWN_SECTION_PATTERNS[section_id]
        if re.search(rf"(?mi)^##+\s+.*(?:{pattern})", text) is None:
            add_issue(issues, f"Missing selected dimension Markdown section: {section_id}")


def finding_chunks(text: str) -> list[str]:
    starts = [m.start() for m in re.finditer(r"(?m)^### Finding:", text)]
    chunks: list[str] = []
    for index, start in enumerate(starts):
        boundary = re.search(r"(?m)^#{1,3}\s+", text[start + 4:])
        end = start + 4 + boundary.start() if boundary else len(text)
        chunks.append(text[start:end])
    return chunks


def lint_markdown_findings(text: str, issues: list[str]) -> None:
    chunks = finding_chunks(text)
    if not chunks:
        return

    for idx, chunk in enumerate(chunks, start=1):
        for field in REQUIRED_FINDING_FIELDS:
            if re.search(rf"(?mi)^-\s*{re.escape(field)}\s*:", chunk) is None:
                add_issue(issues, f"Finding #{idx} missing field: {field}")


def parse_stats_table(text: str) -> dict[str, int] | None:
    stats: dict[str, int] = {}
    for severity in SEVERITIES:
        match = re.search(rf"(?mi)^\|\s*{severity}\s*\|\s*(\d+)\s*\|", text)
        if match:
            stats[severity] = int(match.group(1))
    return stats or None


def lint_markdown_stats(text: str, issues: list[str]) -> None:
    expected = parse_stats_table(text)
    if not expected:
        return

    actual = {severity: 0 for severity in SEVERITIES}
    for match in re.finditer(r"(?mi)^-\s*Severity\s*:\s*(Critical|High|Medium|Low|Info)\b", text):
        actual[match.group(1).title()] += 1

    for severity, expected_count in expected.items():
        if actual[severity] != expected_count:
            add_issue(
                issues,
                f"Severity count mismatch for {severity}: stats table has {expected_count}, detailed findings have {actual[severity]}",
            )

    total_match = re.search(r"(?mi)^\|\s*\*\*Total\*\*\s*\|\s*\*\*(\d+)\*\*\s*\|", text)
    if total_match:
        expected_total = int(total_match.group(1))
        actual_total = sum(actual.values())
        if actual_total != expected_total:
            add_issue(issues, f"Total finding count mismatch: stats table has {expected_total}, detailed findings have {actual_total}")


def lint_json_report(text: str, modes: str | None, issues: list[str]) -> None:
    try:
        report = json.loads(text)
    except (ValueError, RecursionError):
        issues.append("Invalid JSON report (contents omitted)")
        return
    schema = json.loads((Path(__file__).resolve().parents[1] / "templates/audit-report.json").read_text(encoding="utf-8"))
    schema_issues = validate(report, schema)
    issues.extend(schema_issues)
    if schema_issues:
        return
    # Decode escaped JSON strings before scanning; never log the matched values.
    lint_secrets(json.dumps(report, ensure_ascii=False), issues)
    selected = expand_modes(modes or ",".join(report["metadata"]["auditModes"]), issues)
    declared = expand_modes(",".join(report["metadata"]["auditModes"]), issues)
    if modes and set(selected) != set(declared):
        issues.append("Report modes disagree with --modes")
    def canonical(name: str) -> str:
        return re.sub(r"[^a-z0-9]", "", name.lower())
    coverage = {canonical(row["dimension"]) for row in report["coverageMatrix"]}
    sections = {canonical(name) for name in report.get("dimensionSections", {})}
    for mode in selected:
        if canonical(mode) not in coverage:
            issues.append(f"Missing JSON coverage dimension: {mode}")
        if canonical(mode) not in sections:
            issues.append(f"Missing JSON dimension section: {mode}")
    findings = report["detailedFindings"]
    ids = [item["id"] for item in findings]
    if len(ids) != len(set(ids)):
        issues.append("Duplicate finding ids")
    rows = report["findingStatistics"]["bySeverity"]
    if sorted(row["severity"] for row in rows) != sorted(SEVERITIES):
        issues.append("Statistics must contain each severity exactly once")
    for row in [*rows, report["findingStatistics"]["total"]]:
        group = [item for item in findings if "severity" not in row or item["severity"] == row["severity"]]
        if (row["count"] != len(group)
                or row["confirmed"] != sum(item["status"] == "Confirmed" for item in group)
                or row["suspected"] != sum(item["status"] == "Suspected" for item in group)):
            issues.append("JSON finding statistics disagree with detailed findings")
    for section in ("topRisks", "fixOrder", "quickWins"):
        for item in report.get(section, []):
            if "findingId" in item and item["findingId"] not in ids:
                issues.append(f"{section}: unknown finding reference")


def lint_report(path: Path, modes: str | None = None) -> list[str]:
    if path.suffix.lower() not in {".md", ".html", ".htm", ".json"}:
        return ["Unsupported report format"]
    text = path.read_text(encoding="utf-8-sig")
    is_html = path.suffix.lower() in {".html", ".htm"}
    issues: list[str] = []

    lint_placeholders(text, issues)
    lint_secrets(unescape(text) if is_html else text, issues)
    if path.suffix.lower() == ".json":
        lint_json_report(text, modes, issues)
        return issues
    lint_required_sections(text, is_html, issues)
    lint_mode_sections(text, is_html, modes, issues)

    if not is_html:
        lint_markdown_findings(text, issues)
        lint_markdown_stats(text, issues)

    return issues


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Lint generated audit reports for missing structure and unsafe leftovers.")
    parser.add_argument("--modes", help="Comma-separated selected audit modes, for example 'full' or 'security,release'")
    parser.add_argument("reports", nargs="+", type=Path, help="Generated .md, .html, or .json report paths")
    args = parser.parse_args(argv)

    exit_code = 0
    for report in args.reports:
        if not report.is_file():
            print(f"{report}: missing file", file=sys.stderr)
            exit_code = 2
            continue
        try:
            issues = lint_report(report, args.modes)
        except (OSError, UnicodeError, ValueError, RecursionError):
            print(f"{report}: could not read or validate report (contents omitted)", file=sys.stderr)
            exit_code = 2
            continue
        if issues:
            exit_code = max(exit_code, 1)
            print(f"{report}: FAIL")
            for issue in issues:
                print(f"  - {issue}")
        else:
            print(f"{report}: OK")

    return exit_code


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
