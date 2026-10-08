#!/usr/bin/env python3
"""Create a clean distributable zip for the skill directory."""

from __future__ import annotations

import argparse
import fnmatch
import sys
import zipfile
from pathlib import Path


DEFAULT_EXCLUDES = (
    ".DS_Store",
    "*/.DS_Store",
    "README.md",
    "__pycache__/**",
    "*/__pycache__/**",
    "*.pyc",
    "*.pyo",
    ".git/**",
    ".gitignore",
    "dist/**",
)


def should_exclude(relative_path: str, patterns: tuple[str, ...]) -> bool:
    return any(fnmatch.fnmatch(relative_path, pattern) for pattern in patterns)


def iter_package_files(skill_dir: Path, patterns: tuple[str, ...]) -> list[Path]:
    files: list[Path] = []
    for path in sorted(skill_dir.rglob("*")):
        if not path.is_file():
            continue
        relative_path = path.relative_to(skill_dir).as_posix()
        if should_exclude(relative_path, patterns):
            continue
        files.append(path)
    return files


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Package the fuck-my-shit-mountain skill without repository-only files.")
    parser.add_argument("--skill-dir", type=Path, default=Path(__file__).resolve().parents[1], help="Skill directory to package")
    parser.add_argument("--output", type=Path, help="Output zip path. Defaults to <repo>/dist/<skill-name>.zip")
    parser.add_argument("--dry-run", action="store_true", help="Print files that would be included without writing a zip")
    args = parser.parse_args(argv)

    skill_dir = args.skill_dir.resolve()
    if not (skill_dir / "SKILL.md").exists():
        print(f"{skill_dir}: missing SKILL.md", file=sys.stderr)
        return 2

    output = args.output
    if output is None:
        output = skill_dir.parent / "dist" / f"{skill_dir.name}.zip"
    output = output.resolve()

    files = iter_package_files(skill_dir, DEFAULT_EXCLUDES)
    if args.dry_run:
        for path in files:
            print(path.relative_to(skill_dir).as_posix())
        return 0

    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in files:
            archive.write(path, f"{skill_dir.name}/{path.relative_to(skill_dir).as_posix()}")

    print(f"Wrote {output} ({len(files)} files)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
