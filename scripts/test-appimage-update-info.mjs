import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { appImageUpdateInformation } from "./appimage-update-info.mjs";

function selection(info) {
  const [transport, owner, repo, channel, glob] = info.split("|");
  assert.equal(transport, "gh-releases-zsync");
  assert.equal(owner, "muutot");
  assert.equal(repo, "Clipboard");
  assert.equal(channel, "latest");
  const pattern = new RegExp(
    `^${glob
      .split("*")
      .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
      .join(".*")}$`,
  );
  return (asset) => pattern.test(asset);
}

test("updates across versions while excluding other products and architectures", () => {
  for (const version of ["1.8.2", "2.0.0-rc.1", "2.0.0+build.1"]) {
    const accepts = selection(
      appImageUpdateInformation("muutot/Clipboard", `/tmp/Clipboard_${version}_amd64.AppImage`),
    );
    assert.ok(accepts("Clipboard_3.1.0_amd64.AppImage.zsync"));
    assert.ok(!accepts("Clipboard_3.1.0_arm64.AppImage.zsync"));
    assert.ok(!accepts("Other_3.1.0_amd64.AppImage.zsync"));
    assert.ok(!accepts("Clipboard_3.1.0_amd64.AppImage"));
  }
});

test("the real CLI handles Windows paths and preserves the selected architecture", () => {
  const script = fileURLToPath(new URL("./appimage-update-info.mjs", import.meta.url));
  const info = execFileSync(
    process.execPath,
    [script, "muutot/Clipboard", "B:\\fixtures\\Clipboard_1.8.2_aarch64.AppImage"],
    { encoding: "utf8" },
  ).trim();
  const accepts = selection(info);
  assert.ok(accepts("Clipboard_2.0.0_aarch64.AppImage.zsync"));
  assert.ok(!accepts("Clipboard_2.0.0_amd64.AppImage.zsync"));
});

test("rejects malformed repository, unsupported architecture and wildcard injection", () => {
  for (const repository of ["", "owner", "owner/repo|latest|*", "owner/repo/extra"]) {
    assert.throws(() => appImageUpdateInformation(repository, "Clipboard_1.8.2_amd64.AppImage"));
  }
  for (const artifact of [
    "Clipboard.AppImage",
    "Clipboard_1.8.2_mips.AppImage",
    "Clip*_1.8.2_amd64.AppImage",
    "Clip|board_1.8.2_amd64.AppImage",
  ]) {
    assert.throws(() => appImageUpdateInformation("muutot/Clipboard", artifact));
  }
});
