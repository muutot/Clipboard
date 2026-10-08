"""Compile the production NSIS storage resolver and test real config files.

All paths and output files are under one temporary directory. No deletion or
registry operations from the application installer are included in the fixture.
"""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def nsis_string(value: Path) -> str:
    return str(value).replace("$", "$$")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--makensis", type=Path, default=Path(os.environ.get("LOCALAPPDATA", ".")) / "tauri/NSIS/makensis.exe")
    args = parser.parse_args()
    source = (Path(__file__).resolve().parents[1] / "src-tauri/windows/installer.nsi").read_text(encoding="utf-8-sig")
    resolver = re.search(r"^Function un\.ResolveStorageRoot\n.*?^FunctionEnd", source, re.M | re.S)
    if resolver is None:
        raise RuntimeError("Missing production storage resolver")
    helper = re.search(r"^Function un\.WriteStorageResolver\n.*?^FunctionEnd", source, re.M | re.S)
    functions = (helper.group() + "\n" if helper else "") + resolver.group()
    functions = functions.replace("un.", "").replace("${UnStr", "${Str")
    temp_root = Path(tempfile.gettempdir()).resolve()
    with tempfile.TemporaryDirectory(prefix="clipboard-nsis-resolve-", dir=temp_root) as temporary:
        workspace = Path(temporary).resolve()
        assert workspace.parent == temp_root
        cases = []

        def add(name, config, expected=None):
            project = workspace / name
            root = project / "storage"
            marker = root / "database/clipboard.sqlite3"
            marker.parent.mkdir(parents=True)
            marker.write_bytes(b"fixture database")
            if config is not None:
                path = project / "conf/conf.json"
                path.parent.mkdir()
                path.write_text(config, encoding="utf-8")
            if expected is not None and expected != "":
                marker = expected / "database/clipboard.sqlite3"
                marker.parent.mkdir(parents=True, exist_ok=True)
                marker.write_bytes(b"custom fixture database")
            cases.append((name, project, root if expected is None else expected))

        custom = workspace / "custom"
        add("missing-config", None)
        add("compact-slashes", json.dumps({"storage": {"dataDirectory": custom.as_posix()}}, separators=(",", ":")), custom / "storage")
        add("pretty-windows", json.dumps({"storage": {"dataDirectory": str(custom)}}, indent=2), custom / "storage")
        add("multiline-property", '{"storage":{"dataDirectory"\n:\n' + json.dumps(str(custom)) + '}}', custom / "storage")
        unicode_root = workspace / "\u4e2d\u6587 $name's directory"
        add("unicode-path", json.dumps({"storage": {"dataDirectory": str(unicode_root)}}, ensure_ascii=False, indent=2), unicode_root / "storage")
        misleading = workspace / "mystorage"
        (misleading / "files").mkdir(parents=True)
        (misleading / "files/foreign.txt").write_text("foreign", encoding="utf-8")
        add("storage-suffix-parent", json.dumps({"storage": {"dataDirectory": misleading.as_posix()}}, separators=(",", ":")), misleading / "storage")
        add("exact-storage-leaf", json.dumps({"storage": {"dataDirectory": (custom / "storage").as_posix()}}, separators=(",", ":")), custom / "storage")
        add("case-sensitive-leaf", json.dumps({"storage": {"dataDirectory": (custom / "Storage").as_posix()}}, separators=(",", ":")), custom / "Storage/storage")
        add("invalid-json", '{"storage":{"dataDirectory":"broken', "")
        add("unrelated-property", json.dumps({"general": {"dataDirectory": custom.as_posix()}, "storage": {}}, separators=(",", ":")))
        add("wrong-type", '{"storage":{"dataDirectory":42}}', "")
        add("relative-path", '{"storage":{"dataDirectory":"relative"}}', "")
        add("null-directory", '{"storage":{"dataDirectory":null}}')

        executable = workspace / "resolve-test.exe"
        fixture = workspace / "resolve-test.nsi"
        calls = "".join(
            f'StrCpy $INSTDIR "{nsis_string(project)}"\nCall ResolveStorageRoot\nFileOpen $9 "{nsis_string(workspace / (name + ".result"))}" w\nFileWriteUTF16LE $9 "$UninstallStorageRoot"\nFileClose $9\n'
            for name, project, _ in cases
        )
        fixture.write_text(
            'Unicode true\nRequestExecutionLevel user\nSilentInstall silent\n'
            '!include LogicLib.nsh\n!include FileFunc.nsh\n'
            f'OutFile "{nsis_string(executable)}"\nVar UninstallStorageRoot\n{functions}\nSection\n{calls}SectionEnd\n', encoding="utf-8")
        subprocess.run([str(args.makensis.resolve()), "/V2", str(fixture)], check=True, timeout=30)
        subprocess.run([str(executable), "/S"], check=True, timeout=60, creationflags=subprocess.CREATE_NO_WINDOW)
        failures = []
        for name, _, expected in cases:
            actual = (workspace / (name + ".result")).read_bytes().decode("utf-16-le").lstrip("\ufeff")
            correct = actual == "" if expected == "" else bool(actual) and Path(actual).resolve() == expected.resolve()
            if not correct:
                failures.append(name)
        assert not failures, "Incorrect storage resolution: " + ", ".join(failures)
    print(f"PASS: {len(cases)} compiled NSIS storage resolution scenarios")


if __name__ == "__main__":
    main()
