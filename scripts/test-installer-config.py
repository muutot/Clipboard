"""Compile and run NSIS configuration handling against temporary fixtures."""

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
    writer = re.search(r"^Function WriteInitialStorageConfig\n.*?^FunctionEnd", source, re.M | re.S)
    if writer is None:
        raise RuntimeError("Missing initial configuration writer")
    temp_root = Path(tempfile.gettempdir()).resolve()
    with tempfile.TemporaryDirectory(prefix="clipboard-nsis-config-", dir=temp_root) as temporary:
        workspace = Path(temporary).resolve()
        assert workspace.parent == temp_root
        existing = workspace / "existing" / "conf" / "conf.json"
        existing.parent.mkdir(parents=True)
        previous = b'{"general":{"theme":"light"},"storage":{"dataDirectory":"D:/existing"}}'
        existing.write_bytes(previous)
        cases = [(workspace / "fresh", workspace / "data"), (workspace / "existing", workspace / "changed"), (workspace / "unicode", workspace / "\u4e2d\u6587 $name's data")]
        calls = "".join(f'StrCpy $INSTDIR "{nsis_string(project)}"\nStrCpy $DataDirectory "{nsis_string(data)}"\nCall WriteInitialStorageConfig\n' for project, data in cases)
        fixture = workspace / "config-test.nsi"
        executable = workspace / "config-test.exe"
        fixture.write_text(
            'Unicode true\nRequestExecutionLevel user\nSilentInstall silent\n'
            '!include LogicLib.nsh\n!include FileFunc.nsh\n!include StrFunc.nsh\n${StrRep}\n'
            f'OutFile "{nsis_string(executable)}"\nVar DataDirectory\n'
            f'{writer.group()}\nSection\n{calls}SectionEnd\n', encoding="utf-8")
        subprocess.run([str(args.makensis.resolve()), "/V2", str(fixture)], check=True, timeout=30)
        subprocess.run([str(executable), "/S"], check=True, timeout=30, creationflags=subprocess.CREATE_NO_WINDOW)
        failures = []
        if existing.read_bytes() != previous:
            failures.append("reinstallation overwrites existing configuration")
        for project, data in (cases[0], cases[2]):
            try:
                saved = json.loads((project / "conf/conf.json").read_text(encoding="utf-8"))
                if Path(saved["storage"]["dataDirectory"]) != data:
                    failures.append(f"initial directory did not round-trip: {project.name}")
            except (UnicodeError, ValueError, OSError) as error:
                failures.append(f"initial directory is not valid UTF-8 JSON: {project.name}: {error}")
        assert not failures, "; ".join(failures)
    print("PASS: initial NSIS config preserves existing settings and round-trips Unicode paths")


if __name__ == "__main__":
    main()
