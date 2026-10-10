// Section → settings panel registry for the settings dialog.
//
// One homogenized dispatch serves every panel: a descriptor maps section ids
// onto a dynamic import plus a render-time props builder, and the dialog awaits
// through a single generic block. Adding a panel means appending one descriptor
// instead of copying an await block.
//
// The props builders read and write live dialog state, so the registry takes a
// host of accessors instead of owning that state itself.
import type { Component } from "svelte";
import type { StorageStatus } from "$lib/services/storage";
import type { FontSubsection, SettingsSection, StatisticsTab } from "$lib/settings-navigation";

export type LazyPanelModule = { default: Component<any> };

export interface LazyPanelDescriptor {
  sections: readonly string[];
  load(): Promise<LazyPanelModule>;
  props(): Record<string, unknown>;
  /** Shared module-cache key when one panel serves several sections. */
  cacheKey?: string;
}

/** Live dialog state the descriptors' props builders read or write. */
export interface LazyPanelHost {
  readonly onclose: () => void;
  readonly status: StorageStatus | null;
  readonly activeSection: SettingsSection;
  readonly activeStatisticsTab: StatisticsTab;
  readonly activeFontSection: FontSubsection;
  readonly tagSearch: string;
  readonly loading: boolean;
  readonly appVersion: string;
  readonly appExecutablePath: string;
  setActiveSection(section: SettingsSection): void;
  setFontSection(section: FontSubsection): void;
  setTagSearch(value: string): void;
  showFeedback(message: string, success: boolean): void;
  refreshStorageStats(): Promise<void>;
}

export interface LazyPanelRegistry {
  /** Descriptors in declaration order; the dialog resolves the active one. */
  readonly descriptors: readonly LazyPanelDescriptor[];
  /** Cached module for a section, importing it on first request. */
  load(section: string): Promise<LazyPanelModule>;
}

export function createLazyPanelRegistry(host: LazyPanelHost): LazyPanelRegistry {
  const modules = new Map<string, Promise<LazyPanelModule>>();

  const descriptors: LazyPanelDescriptor[] = [
    {
      sections: ["general_general"],
      load: () => import("$lib/components/settings/GeneralGeneralSettingsPanel.svelte"),
      props: () => ({ onclose: host.onclose, showHeader: false }),
    },
    {
      sections: ["general_window"],
      load: () => import("$lib/components/settings/GeneralWindowSettingsPanel.svelte"),
      props: () => ({ onclose: host.onclose, showHeader: false }),
    },
    {
      sections: ["general_search"],
      load: () => import("$lib/components/settings/GeneralSearchSettingsPanel.svelte"),
      props: () => ({ onclose: host.onclose, showHeader: false }),
    },
    {
      sections: ["general_items"],
      load: () => import("$lib/components/settings/GeneralItemsSettingsPanel.svelte"),
      props: () => ({ onclose: host.onclose, showHeader: false }),
    },
    {
      sections: ["layout"],
      load: () => import("$lib/components/settings/LayoutSettingsPanel.svelte"),
      props: () => ({ onclose: host.onclose, showHeader: false }),
    },
    {
      sections: ["font"],
      load: () => import("$lib/components/settings/FontSizeSettingsPanel.svelte"),
      props: () => ({
        onclose: host.onclose,
        showHeader: false,
        fontSection: host.activeFontSection,
        onselectfontsection: (section: FontSubsection) => host.setFontSection(section),
      }),
    },
    {
      sections: ["theme"],
      load: () => import("$lib/components/settings/ThemeSettingsPanel.svelte"),
      props: () => ({ onclose: host.onclose, showHeader: false }),
    },
    {
      sections: ["custom_css"],
      load: () => import("$lib/components/settings/CustomCssSettingsPanel.svelte"),
      props: () => ({ onclose: host.onclose, showHeader: false }),
    },
    {
      sections: ["icons"],
      load: () => import("$lib/components/settings/IconColorsSettingsPanel.svelte"),
      props: () => ({ onclose: host.onclose, showHeader: false }),
    },
    {
      sections: ["capture"],
      load: () => import("$lib/components/settings/IgnoredAppsSettingsPanel.svelte"),
      props: () => ({ iconsDir: host.status?.iconsDir, onclose: host.onclose, showHeader: false }),
    },
    {
      sections: ["capture_privacy"],
      load: () => import("$lib/components/settings/SensitiveContentSettingsPanel.svelte"),
      props: () => ({ onclose: host.onclose, showHeader: false }),
    },
    {
      sections: ["capture_icons"],
      load: () => import("$lib/components/settings/IconCacheSettingsPanel.svelte"),
      props: () => ({
        iconsDir: host.status?.iconsDir ?? "",
        onfeedback: host.showFeedback,
      }),
    },
    {
      sections: ["tags"],
      load: () => import("$lib/components/settings/TagManagementSettingsPanel.svelte"),
      props: () => ({
        onclose: host.onclose,
        showHeader: false,
        tagSearch: host.tagSearch,
        ontagSearchChange: host.setTagSearch,
      }),
    },
    {
      sections: ["tags_rules"],
      load: () => import("$lib/components/settings/TagRulesSettingsPanel.svelte"),
      props: () => ({
        onclose: host.onclose,
        showHeader: false,
      }),
    },
    {
      sections: ["ocr"],
      load: () => import("$lib/components/settings/OcrSettingsPanel.svelte"),
      props: () => ({
        onfeedback: host.showFeedback,
      }),
    },
    {
      sections: ["storage_paths"],
      load: () => import("$lib/components/settings/StoragePathsPanel.svelte"),
      props: () => ({
        status: host.status as StorageStatus,
        onfeedback: host.showFeedback,
      }),
    },
    {
      sections: ["storage_limits"],
      load: () => import("$lib/components/settings/StorageLimitsPanel.svelte"),
      props: () => ({
        onfeedback: host.showFeedback,
      }),
    },
    {
      sections: ["storage_tools"],
      load: () => import("$lib/components/settings/StorageToolsPanel.svelte"),
      props: () => ({
        onfeedback: host.showFeedback,
        onadjustlimit: () => host.setActiveSection("storage_limits"),
      }),
    },
    {
      sections: ["sync_cloud"],
      cacheKey: "sync",
      load: () => import("$lib/components/settings/SyncPanel.svelte"),
      props: () => ({
        tab: "cloud" as const,
        onfeedback: host.showFeedback,
      }),
    },
    {
      sections: ["sync_advanced"],
      cacheKey: "sync",
      load: () => import("$lib/components/settings/SyncPanel.svelte"),
      props: () => ({
        tab: "advanced" as const,
        onfeedback: host.showFeedback,
      }),
    },
    {
      sections: ["sync_s3"],
      cacheKey: "sync",
      load: () => import("$lib/components/settings/SyncPanel.svelte"),
      props: () => ({
        tab: "s3" as const,
        onfeedback: host.showFeedback,
      }),
    },
    {
      sections: ["keyboard_item", "keyboard_quick", "keyboard_system", "keyboard_switch"],
      load: () => import("$lib/components/settings/KeyboardSettingsPanel.svelte"),
      props: () => ({
        onclose: host.onclose,
        category: host.activeSection.startsWith("keyboard_")
          ? (host.activeSection.slice("keyboard_".length) as "item" | "quick" | "system" | "switch")
          : "item",
        showHeader: false,
        configPath: host.status?.keyboardConfigPath ?? null,
      }),
    },
    {
      sections: ["keyboard_float"],
      load: () => import("$lib/components/settings/FloatPanelClickSettingsPanel.svelte"),
      props: () => ({
        showHeader: false,
      }),
    },
    {
      sections: ["statistics"],
      load: () => import("$lib/components/settings/StatisticsSettingsPanel.svelte"),
      props: () => ({
        activeTab: host.activeStatisticsTab,
        status: host.status,
        loading: host.loading,
        onrefreshStatus: host.refreshStorageStats,
        onclose: host.onclose,
      }),
    },
    {
      sections: ["about"],
      load: () => import("$lib/components/settings/AboutSettingsPanel.svelte"),
      props: () => ({
        appVersion: host.appVersion,
        appExecutablePath: host.appExecutablePath,
        onclose: host.onclose,
      }),
    },
  ];

  function load(section: string): Promise<LazyPanelModule> {
    const descriptor = descriptors.find((entry) => entry.sections.includes(section));
    if (!descriptor) return Promise.reject(new Error(`no panel for ${section}`));
    const cacheKey = descriptor.cacheKey ?? section;
    let promise = modules.get(cacheKey);
    if (!promise) {
      promise = descriptor.load();
      // Evict on rejection: a cached rejected promise would make every later
      // navigation re-await the same failure, permanently breaking the panel
      // after one transient chunk-load error until the window restarts.
      promise.catch(() => modules.delete(cacheKey));
      modules.set(cacheKey, promise);
    }
    return promise;
  }

  return { descriptors, load };
}
