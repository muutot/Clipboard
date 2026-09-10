import { invokeTauriRequired } from "$lib/services/runtime";

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
  return invokeTauriRequired<UpdateInfo>(
    "check_for_update",
    undefined,
    "Update checking is only available in the desktop app",
  );
}

export async function getRelease(version: string): Promise<UpdateInfo> {
  return invokeTauriRequired<UpdateInfo>(
    "get_release",
    { version },
    "Release lookup is only available in the desktop app",
  );
}
