import { invokeTauri, invokeTauriRequired } from "$lib/services/runtime";

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
  return invokeTauri<ApplicationFilterSettings>("get_application_filter_settings");
}

export async function configureIgnoredApplications(applications: string[]): Promise<string[]> {
  return invokeTauriRequired<string[]>(
    "configure_ignored_applications",
    { applications },
    "Application filters are only available in the desktop app",
  );
}

export interface PrivacySettings {
  paused: boolean;
  localOnly: boolean;
  sensitivePatterns: string[];
}

export async function getPrivacySettings(): Promise<PrivacySettings> {
  return invokeTauri<PrivacySettings>("get_privacy_settings", undefined, {
    paused: false,
    localOnly: true,
    sensitivePatterns: [],
  });
}

export async function setPrivacySettings(settings: {
  localOnly?: boolean;
  sensitivePatterns?: string[];
}): Promise<PrivacySettings> {
  return invokeTauriRequired<PrivacySettings>(
    "set_privacy_settings",
    settings,
    "Privacy settings are only available in the desktop app",
  );
}
