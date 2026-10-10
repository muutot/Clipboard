// Settings-search controller for the settings dialog shell.
//
// Owns the query state, the filtered result list, lookup of a mounted result
// card, and the highlight/scroll jump. Section selection stays with the dialog,
// so jumping calls back instead of writing dialog state from here.
import { tick } from "svelte";
import {
  resolveSettingsNavPath,
  type FontSubsection,
  type SettingsSection,
  type StatisticsTab,
} from "$lib/settings-navigation";
import {
  filterSettingsSearchItems,
  normalizeSettingsSearch,
  resolveSettingsSearchItems,
  type SettingsSearchItem,
} from "$lib/settings-search";

export interface SettingsSearchDeps {
  /** The dialog's scroll container; reactive so the observer re-attaches. */
  readonly contentEl: HTMLElement | null;
  /** Read inside the count effect so switching sections re-counts the cards. */
  readonly activeSection: SettingsSection;
  readonly activeStatisticsTab: StatisticsTab;
  translate(key: string): string;
  selectSection(section: SettingsSection, statisticsTab?: StatisticsTab): void;
  selectFontSection(section: FontSubsection): void;
}

export interface SettingsSearchController {
  query: string;
  readonly itemCount: number;
  readonly active: boolean;
  readonly results: SettingsSearchItem[];
  readonly totalItems: number;
  clear(): void;
  resultPath(item: SettingsSearchItem): string;
  openResult(item: SettingsSearchItem): Promise<void>;
}

export function createSettingsSearchController(deps: SettingsSearchDeps): SettingsSearchController {
  let query = $state("");
  let itemCount = $state(0);
  let highlightedItem: HTMLElement | null = null;
  let highlightTimer: ReturnType<typeof setTimeout> | undefined;

  const resolvedItems = $derived.by(() => resolveSettingsSearchItems((key) => deps.translate(key)));
  const normalizedQuery = $derived(normalizeSettingsSearch(query));
  const active = $derived(Boolean(normalizedQuery));
  const results = $derived.by(() =>
    normalizedQuery ? filterSettingsSearchItems(resolvedItems, normalizedQuery) : [],
  );

  function elementText(item: HTMLElement): string {
    const labels = item.querySelectorAll<HTMLElement>(
      "strong, p, label, .setting-label, .config-path, .column-heading, code",
    );
    const text = Array.from(labels)
      .map((element) => element.textContent ?? "")
      .join(" ");
    return normalizeSettingsSearch(text || item.textContent || "");
  }

  function currentElements(): HTMLElement[] {
    if (!deps.contentEl) return [];
    return Array.from(
      deps.contentEl.querySelectorAll<HTMLElement>(
        ".settings-scroll .setting-card, .settings-scroll .filter-board",
      ),
    );
  }

  function updateItemCount(): void {
    itemCount = currentElements().length;
  }

  function clear(): void {
    query = "";
  }

  function resultPath(item: SettingsSearchItem): string {
    return resolveSettingsNavPath(deps.translate, item.section, item.statisticsTab).join(" / ");
  }

  function findElement(item: SettingsSearchItem): HTMLElement | null {
    if (deps.contentEl) {
      const byId = deps.contentEl.querySelector<HTMLElement>(
        `[data-settings-search-id="${item.id}"]`,
      );
      if (byId) return byId;
    }
    const title = normalizeSettingsSearch(item.title);
    const elements = currentElements();
    const match =
      elements.find((element) => {
        const heading = element.querySelector<HTMLElement>(
          "strong, .setting-label, .column-heading",
        );
        return normalizeSettingsSearch(heading?.textContent ?? "") === title;
      }) ??
      elements.find((element) => elementText(element).includes(title)) ??
      null;
    if (match) return match;
    const header = deps.contentEl?.querySelector<HTMLElement>(".settings-section-header");
    if (header && normalizeSettingsSearch(header.textContent ?? "").includes(title)) return header;
    return null;
  }

  function highlight(element: HTMLElement): void {
    if (highlightTimer !== undefined) clearTimeout(highlightTimer);
    highlightedItem?.classList.remove("settings-search-target-highlight");
    highlightedItem = element;
    element.classList.add("settings-search-target-highlight");
    element.scrollIntoView({ behavior: "smooth", block: "center" });
    highlightTimer = setTimeout(() => {
      element.classList.remove("settings-search-target-highlight");
      if (highlightedItem === element) highlightedItem = null;
      highlightTimer = undefined;
    }, 1800);
  }

  function waitForElement(item: SettingsSearchItem, timeout = 2000): Promise<HTMLElement | null> {
    const deadline = Date.now() + timeout;
    return new Promise((resolve) => {
      const poll = () => {
        const element = findElement(item);
        if (element || Date.now() >= deadline) {
          resolve(element);
          return;
        }
        setTimeout(poll, 60);
      };
      poll();
    });
  }

  async function openResult(item: SettingsSearchItem): Promise<void> {
    deps.selectSection(item.section, item.statisticsTab);
    if (item.fontSection) deps.selectFontSection(item.fontSection);
    query = "";
    await tick();
    await tick();
    updateItemCount();
    const element = await waitForElement(item);
    if (element) highlight(element);
  }

  $effect(() => {
    const root = deps.contentEl;
    if (!root || typeof MutationObserver === "undefined") return;

    updateItemCount();
    const observer = new MutationObserver(() => updateItemCount());
    observer.observe(root, { childList: true, subtree: true });
    return () => observer.disconnect();
  });

  $effect(() => {
    deps.activeSection;
    deps.activeStatisticsTab;
    void tick().then(() => updateItemCount());
  });

  return {
    get query() {
      return query;
    },
    set query(next: string) {
      query = next;
    },
    get itemCount() {
      return itemCount;
    },
    get active() {
      return Boolean(normalizedQuery);
    },
    get results() {
      return results;
    },
    get totalItems() {
      return resolvedItems.length;
    },
    clear,
    resultPath,
    openResult,
  };
}
