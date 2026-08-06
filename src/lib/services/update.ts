import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/services/runtime";

export interface UpdateInfo {
  currentVersion: string;
  latestVersion: string;
  updateAvailable: boolean;
  releaseUrl: string;
  releaseTitle: string | null;
  releaseNotes: string | null;
  publishedAt: string | null;
}

export async function checkForUpdate(): Promise<UpdateInfo> {
  if (!isTauriRuntime()) {
    throw new Error("Update checking is only available in the desktop app");
  }

  return invoke<UpdateInfo>("check_for_update");
}

export async function getRelease(version: string): Promise<UpdateInfo> {
  if (!isTauriRuntime()) {
    throw new Error("Release lookup is only available in the desktop app");
  }

  return invoke<UpdateInfo>("get_release", { version });
}
