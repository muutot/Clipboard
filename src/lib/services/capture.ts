import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/services/runtime";

export interface ApplicationFilterSettings {
  discoveredApplications: string[];
  ignoredApplications: string[];
}

export async function getApplicationFilterSettings(): Promise<ApplicationFilterSettings | null> {
  if (!isTauriRuntime()) return null;

  return invoke<ApplicationFilterSettings>("get_application_filter_settings");
}

export async function configureIgnoredApplications(applications: string[]): Promise<string[]> {
  if (!isTauriRuntime()) {
    throw new Error("Application filters are only available in the desktop app");
  }

  const result = await invoke<string[]>("configure_ignored_applications", { applications });
  invoke("set_clipboard_ignored_apps", { apps: applications }).catch(() => {});
  return result;
}
