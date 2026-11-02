import { basename, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** Select the latest release for this product and architecture, across versions. */
export function appImageUpdateInformation(repository, appImage) {
  if (!/^[A-Za-z0-9-]+\/[A-Za-z0-9_.-]+$/.test(repository)) {
    throw new Error("Expected a GitHub owner/repository");
  }
  const name = basename(appImage.replaceAll("\\", "/"));
  const artifact =
    /^([A-Za-z0-9_.-]+)_\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?_(amd64|x86_64|arm64|aarch64)\.AppImage$/.exec(
      name,
    );
  if (!artifact) throw new Error("Expected a versioned AppImage with a supported architecture");
  const [owner, repo] = repository.split("/");
  return `gh-releases-zsync|${owner}|${repo}|latest|${artifact[1]}_*_${artifact[2]}.AppImage.zsync`;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 4)
    throw new Error("Usage: appimage-update-info.mjs owner/repo AppImage");
  console.log(appImageUpdateInformation(process.argv[2], process.argv[3]));
}
