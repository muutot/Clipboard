"""Run the installer's actual settings cleanup in isolated NSIS fixtures.

Windows only. Pass --makensis to override the Tauri cached NSIS compiler.
No application build, installation, registry write, or real user data is used.
"""

import argparse
import os
from pathlib import Path
import re
import subprocess
import tempfile


def nsis_string(value: Path) -> str:
    return str(value).replace("$", "$$")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--makensis",
        type=Path,
        default=Path(os.environ.get("LOCALAPPDATA", ".")) / "tauri/NSIS/makensis.exe",
    )
    args = parser.parse_args()
    source = (Path(__file__).resolve().parents[1] / "src-tauri/windows/installer.nsi").read_text(encoding="utf-8-sig")
    match = re.search(r"^Function un\.DeleteSettingsAt\n.*?^FunctionEnd", source, re.M | re.S)
    if match is None:
        raise RuntimeError("Missing settings cleanup function")
    function = match.group().replace("Function un.DeleteSettingsAt", "Function DeleteSettingsAt", 1)

    temp_root = Path(tempfile.gettempdir()).resolve()
    with tempfile.TemporaryDirectory(prefix="clipboard-nsis-settings-", dir=temp_root) as temporary:
        workspace = Path(temporary).resolve()
        assert workspace.parent == temp_root
        data = workspace / "application-data"
        protected = ["storage/database/clipboard.sqlite3", "storage/image/keep.png", "storage/models/keep.onnx"]
        settings = ["conf/conf.json", "logs/app.log", "instance.lock", "EBWebView/cache.bin"]
        for name in protected + settings:
            path = data / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(name.encode())
        executable = workspace / "cleanup-test.exe"
        fixture = workspace / "cleanup-test.nsi"
        fixture.write_text(
            'Unicode true\nRequestExecutionLevel user\nSilentInstall silent\n'
            f'OutFile "{nsis_string(executable)}"\n'
            f'{function}\nSection\nPush "{nsis_string(data)}"\nCall DeleteSettingsAt\nSectionEnd\n',
            encoding="utf-8",
        )
        subprocess.run([str(args.makensis.resolve()), "/V2", str(fixture)], check=True, timeout=30)
        subprocess.run([str(executable), "/S"], check=True, timeout=30, creationflags=subprocess.CREATE_NO_WINDOW)
        for name in protected:
            assert (data / name).read_bytes() == name.encode(), f"Settings cleanup deleted data: {name}"
        for name in settings:
            assert not (data / name).exists(), f"Settings cleanup retained settings: {name}"
    print("PASS: compiled NSIS settings cleanup preserves history, resources, and models")


if __name__ == "__main__":
    main()
