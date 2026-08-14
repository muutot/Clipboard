import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/services/runtime";

export interface DiscoveredApplication {
  name: string;
  iconPath: string | null;
}

export interface ApplicationFilterSettings {
  discoveredApplications: string[];
  discoveredApplicationsWithIcons: DiscoveredApplication[];
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

  return invoke<string[]>("configure_ignored_applications", { applications });
}

export interface PrivacySettings {
  paused: boolean;
  localOnly: boolean;
  captureSensitiveSources: boolean;
  sensitivePatterns: string[];
  passwordManagerApps: string[];
}

export async function getPrivacySettings(): Promise<PrivacySettings> {
  if (!isTauriRuntime()) {
    return {
      paused: false,
      localOnly: true,
      captureSensitiveSources: false,
      sensitivePatterns: [],
      passwordManagerApps: [],
    };
  }

  return invoke<PrivacySettings>("get_privacy_settings");
}

export async function setPrivacySettings(settings: {
  localOnly?: boolean;
  captureSensitiveSources?: boolean;
  sensitivePatterns?: string[];
}): Promise<PrivacySettings> {
  if (!isTauriRuntime()) {
    throw new Error("Privacy settings are only available in the desktop app");
  }

  return invoke<PrivacySettings>("set_privacy_settings", settings);
}
