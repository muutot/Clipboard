import { describe, expect, it } from "vitest";
import { resolveSettingsNavPath } from "./settings-navigation";

const t = (key: string) => key;

describe("resolveSettingsNavPath", () => {
  it("returns two-level crumbs for every multi-tab group and follows tab switches", () => {
    expect(resolveSettingsNavPath(t, "general_general")).toEqual([
      "storage.generalTab",
      "storage.generalGeneralTab",
    ]);
    expect(resolveSettingsNavPath(t, "general_window")).toEqual([
      "storage.generalTab",
      "storage.generalWindowTab",
    ]);
    expect(resolveSettingsNavPath(t, "storage_paths")).toEqual([
      "storage.storageTab",
      "storage.storagePathsTab",
    ]);
    expect(resolveSettingsNavPath(t, "storage_limits")).toEqual([
      "storage.storageTab",
      "storage.storageLimitsTab",
    ]);
    expect(resolveSettingsNavPath(t, "storage_tools")).toEqual([
      "storage.storageTab",
      "storage.storageToolsTab",
    ]);
    expect(resolveSettingsNavPath(t, "sync_cloud")).toEqual([
      "storage.syncTab",
      "storage.syncCloudTab",
    ]);
    expect(resolveSettingsNavPath(t, "sync_advanced")).toEqual([
      "storage.syncTab",
      "storage.syncAdvancedTab",
    ]);
    expect(resolveSettingsNavPath(t, "sync_s3")).toEqual(["storage.syncTab", "storage.syncS3Tab"]);
    expect(resolveSettingsNavPath(t, "capture_privacy")).toEqual([
      "storage.captureTab",
      "capture.sensitiveContentTitle",
    ]);
    expect(resolveSettingsNavPath(t, "statistics", "memory")).toEqual([
      "storage.statisticsTab",
      "statistics.memoryTab",
    ]);
  });

  it("switches crumbs when moving between groups", () => {
    const before = resolveSettingsNavPath(t, "storage_paths");
    const after = resolveSettingsNavPath(t, "capture");
    expect(before[0]).not.toBe(after[0]);
    expect(after).toEqual(["storage.captureTab", "capture.title"]);
  });

  it("collapses single-tab groups to the group label alone", () => {
    expect(resolveSettingsNavPath(t, "ocr")).toEqual(["OCR"]); // displayLabel
    expect(resolveSettingsNavPath(t, "tags")).toEqual(["storage.tagsTab"]);
    expect(resolveSettingsNavPath(t, "about")).toEqual(["about.tabLabel"]);
  });

  it("never returns a stale storage crumb for non-storage sections", () => {
    for (const section of [
      "general_general",
      "theme",
      "capture_icons",
      "sync_cloud",
      "keyboard_item",
      "ocr",
      "statistics",
      "about",
    ] as const) {
      const path = resolveSettingsNavPath(t, section);
      expect(path.join("/")).not.toContain("storage.storagePathsTab");
    }
  });
});
