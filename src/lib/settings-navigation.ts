import type { IconName } from "$lib/types/clipboard";

export const SETTINGS_SECTIONS = [
  "general_search",
  "general_items",
  "general_window",
  "general_general",
  "compact",
  "font",
  "theme",
  "icons",
  "capture",
  "capture_icons",
  "storage_paths",
  "storage_limits",
  "storage_tools",
  "sync_cloud",
  "sync_advanced",
  "keyboard_item",
  "keyboard_quick",
  "keyboard_system",
  "tags",
  "ocr",
  "statistics",
  "about",
] as const;

export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

export const STATISTICS_TABS = ["storage", "performance", "memory"] as const;

export type StatisticsTab = (typeof STATISTICS_TABS)[number];

export type SettingsNavGroupId =
  | "general"
  | "appearance"
  | "capture"
  | "storage"
  | "sync"
  | "keyboard"
  | "tags"
  | "ocr"
  | "statistics"
  | "about";

export interface SettingsNavTargetDefinition {
  section: SettingsSection;
  statisticsTab?: StatisticsTab;
  labelKey: string;
  titleKey?: string;
  descriptionKey?: string;
}

export interface SettingsNavGroupDefinition {
  id: SettingsNavGroupId;
  icon: IconName;
  labelKey: string;
  displayLabel?: string;
  ariaLabelKey?: string;
  breadcrumbLabelKey?: string;
  includeTabInBreadcrumb?: boolean;
  preserveTabOnPrimary?: boolean;
  tabs: readonly SettingsNavTargetDefinition[];
}

export const SETTINGS_NAV_GROUP_DEFINITIONS: readonly SettingsNavGroupDefinition[] = [
  {
    id: "general",
    icon: "sliders",
    labelKey: "storage.generalTab",
    includeTabInBreadcrumb: true,
    tabs: [
      {
        section: "general_general",
        labelKey: "storage.generalGeneralTab",
        descriptionKey: "storage.generalGeneralDescription",
      },
      {
        section: "general_window",
        labelKey: "storage.generalWindowTab",
        descriptionKey: "storage.generalWindowDescription",
      },
      {
        section: "general_search",
        labelKey: "storage.generalSearchTab",
        descriptionKey: "storage.generalSearchDescription",
      },
      {
        section: "general_items",
        labelKey: "storage.generalItemsTab",
        descriptionKey: "storage.generalItemsDescription",
      },
    ],
  },
  {
    id: "appearance",
    icon: "palette",
    labelKey: "storage.appearanceTab",
    includeTabInBreadcrumb: true,
    tabs: [
      {
        section: "theme",
        labelKey: "storage.themeTab",
        descriptionKey: "general.fontSizeDescription",
      },
      {
        section: "font",
        labelKey: "storage.fontTab",
        descriptionKey: "general.fontSizeDescription",
      },
      {
        section: "compact",
        labelKey: "storage.compactTab",
        descriptionKey: "compact.description",
      },
      {
        section: "icons",
        labelKey: "storage.iconsTab",
        descriptionKey: "general.iconColorsDescription",
      },
    ],
  },
  {
    id: "capture",
    icon: "filter",
    labelKey: "storage.captureTab",
    tabs: [
      {
        section: "capture",
        labelKey: "capture.title",
        descriptionKey: "capture.description",
      },
      {
        section: "capture_icons",
        labelKey: "storage.iconCacheTitle",
        descriptionKey: "storage.iconCacheDesc",
      },
    ],
  },
  {
    id: "storage",
    icon: "file",
    labelKey: "storage.storageTab",
    breadcrumbLabelKey: "storage.storageToolsTab",
    tabs: [
      {
        section: "storage_paths",
        labelKey: "storage.storagePathsTab",
        descriptionKey: "storage.storagePathsDescription",
      },
      {
        section: "storage_limits",
        labelKey: "storage.storageLimitsTab",
        descriptionKey: "storage.storageLimitsDescription",
      },
      {
        section: "storage_tools",
        labelKey: "storage.storageToolsTab",
        descriptionKey: "storage.storageToolsDescription",
      },
    ],
  },
  {
    id: "sync",
    icon: "cloud",
    labelKey: "storage.syncTab",
    tabs: [
      {
        section: "sync_cloud",
        labelKey: "storage.syncCloudTab",
        titleKey: "storage.syncTitle",
        descriptionKey: "storage.syncDescription",
      },
      {
        section: "sync_advanced",
        labelKey: "storage.syncAdvancedTab",
        titleKey: "storage.syncTitle",
        descriptionKey: "storage.syncDescription",
      },
    ],
  },
  {
    id: "keyboard",
    icon: "keyboard",
    labelKey: "storage.keyboardTab",
    tabs: [
      {
        section: "keyboard_item",
        labelKey: "storage.keyboardItemTab",
        titleKey: "keyboard.title",
        descriptionKey: "storage.keyboardItemDescription",
      },
      {
        section: "keyboard_quick",
        labelKey: "storage.keyboardQuickTab",
        titleKey: "keyboard.title",
        descriptionKey: "storage.keyboardQuickDescription",
      },
      {
        section: "keyboard_system",
        labelKey: "storage.keyboardSystemTab",
        titleKey: "keyboard.title",
        descriptionKey: "storage.keyboardSystemDescription",
      },
    ],
  },
  {
    id: "tags",
    icon: "tag",
    labelKey: "storage.tagsTab",
    tabs: [{ section: "tags", labelKey: "storage.tagsSectionTitle" }],
  },
  {
    id: "ocr",
    icon: "eye",
    labelKey: "storage.ocrTitle",
    displayLabel: "OCR",
    tabs: [
      {
        section: "ocr",
        labelKey: "storage.ocrTitle",
        descriptionKey: "storage.ocrDescription",
      },
    ],
  },
  {
    id: "statistics",
    icon: "bar-chart",
    labelKey: "storage.statisticsTab",
    ariaLabelKey: "statistics.title",
    includeTabInBreadcrumb: true,
    preserveTabOnPrimary: true,
    tabs: [
      {
        section: "statistics",
        statisticsTab: "storage",
        labelKey: "statistics.storageTab",
        descriptionKey: "statistics.storageDescription",
      },
      {
        section: "statistics",
        statisticsTab: "performance",
        labelKey: "statistics.performanceTab",
        descriptionKey: "statistics.performanceDescription",
      },
      {
        section: "statistics",
        statisticsTab: "memory",
        labelKey: "statistics.memoryTab",
        descriptionKey: "statistics.memoryDescription",
      },
    ],
  },
  {
    id: "about",
    icon: "info",
    labelKey: "about.tabLabel",
    tabs: [{ section: "about", labelKey: "about.sectionTitle" }],
  },
];

export type SettingsNavigationTranslate = (key: string) => string;

export function resolveSettingsNavPath(
  translate: SettingsNavigationTranslate,
  section: SettingsSection,
  statisticsTab?: StatisticsTab,
): string[] {
  for (const group of SETTINGS_NAV_GROUP_DEFINITIONS) {
    const tab = group.tabs.find(
      (candidate) =>
        candidate.section === section &&
        (candidate.statisticsTab === undefined ||
          candidate.statisticsTab === (statisticsTab ?? "storage")),
    );
    if (!tab) continue;

    const groupLabel = group.displayLabel ?? translate(group.breadcrumbLabelKey ?? group.labelKey);
    return group.includeTabInBreadcrumb ? [groupLabel, translate(tab.labelKey)] : [groupLabel];
  }

  return [section];
}
