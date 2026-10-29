// Web-source display-name resolution.
//
// Browser captures carry a `web` metadata blob (page title, URL, domain). The
// card/detail views show a short display name instead of the raw page title,
// resolved as: a user mapping rule (domain or title keyword) → a built-in
// well-known-domain name → a truncated page title → the source app name.
//
// Rules live in the sourceMapping config group (edited in the Capture → Web
// source settings panel). Every window subscribes before loading them, then
// re-reads the persisted rules after a save notification.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getSourceMappingRules, type SourceMappingRule } from "$lib/services/clipboard";
import { isTauriRuntime } from "$lib/services/runtime";
import { BUILTIN_DOMAIN_NAMES } from "./source-mapping-domains";
import type { WebSourceMetadata } from "$lib/types/clipboard";

/** Cap for raw page titles shown when no rule matches. */
const WEB_TITLE_MAX_LENGTH = 24;

function matchesDomain(domain: string, pattern: string): boolean {
  const normalized = pattern.trim().toLocaleLowerCase();
  return domain === normalized || domain.endsWith(`.${normalized}`);
}

function truncateTitle(title: string): string {
  const trimmed = title.trim();
  const chars = Array.from(trimmed);
  if (chars.length <= WEB_TITLE_MAX_LENGTH) return trimmed;
  return `${chars.slice(0, WEB_TITLE_MAX_LENGTH).join("")}…`;
}

interface WebSourceItem {
  sourceApp: string;
  webSource?: WebSourceMetadata;
}

/** Resolves the first user rule, built-in name, page title, or source app. */
function resolveName(item: WebSourceItem, rules: SourceMappingRule[]): string {
  const web = item.webSource;
  if (!web) return item.sourceApp;

  for (const rule of rules) {
    if (rule.name && rule.domain && web.domain && matchesDomain(web.domain, rule.domain)) {
      return rule.name;
    }
    if (rule.name && rule.titleKeyword && web.title?.includes(rule.titleKeyword)) {
      return rule.name;
    }
  }
  if (web.domain) {
    for (const [domain, name] of BUILTIN_DOMAIN_NAMES) {
      if (matchesDomain(web.domain, domain)) return name;
    }
  }
  if (web.title) return truncateTitle(web.title);
  return item.sourceApp;
}

/** One window's reactive cache, with a listener and reads owned by its lifecycle. */
export function createSourceMappingStore() {
  let rules = $state.raw<SourceMappingRule[]>([]);
  let loaded = false;
  let lifecycle = 0;
  let refreshRequested = false;
  let unlisten: UnlistenFn | undefined;
  let pendingListen: Promise<void> | undefined;
  let pendingRead: Promise<void> | undefined;

  function reload(): Promise<void> {
    loaded = false;
    refreshRequested = true;
    if (pendingRead) return pendingRead;
    const currentLifecycle = lifecycle;
    pendingRead = Promise.resolve().then(async () => {
      try {
        while (currentLifecycle === lifecycle && refreshRequested) {
          refreshRequested = false;
          try {
            const saved = await getSourceMappingRules();
            if (currentLifecycle !== lifecycle) return;
            // A notification during this read invalidates its snapshot. Read
            // again before publishing, and let every awaiting caller settle on it.
            if (!refreshRequested) {
              rules = (saved ?? []).map((rule) => ({ ...rule }));
              loaded = true;
            }
          } catch (error) {
            if (currentLifecycle !== lifecycle) return;
            if (!refreshRequested) throw error;
          }
        }
      } finally {
        if (currentLifecycle === lifecycle) pendingRead = undefined;
      }
    });
    return pendingRead;
  }

  function ensureListener(): Promise<void> {
    if (!isTauriRuntime() || unlisten) return Promise.resolve();
    if (pendingListen) return pendingListen;
    const currentLifecycle = lifecycle;
    pendingListen = listen<null>("source-mapping-rules-changed", () => {
      if (currentLifecycle !== lifecycle) return;
      void reload().catch((error) =>
        console.error("Unable to refresh source mapping rules", error),
      );
    })
      .then((release) => {
        if (currentLifecycle !== lifecycle) release();
        else {
          unlisten = release;
          // Changes may have been missed while registration was unavailable.
          loaded = false;
        }
      })
      .finally(() => {
        if (currentLifecycle === lifecycle) pendingListen = undefined;
      });
    return pendingListen;
  }

  async function prepareListener(): Promise<void> {
    const currentLifecycle = lifecycle;
    try {
      await ensureListener();
    } catch (error) {
      if (currentLifecycle === lifecycle) {
        console.error("Unable to listen for source mapping changes", error);
      }
    }
  }

  async function initialize(): Promise<void> {
    const currentLifecycle = lifecycle;
    await prepareListener();
    if (currentLifecycle !== lifecycle || loaded) return;
    try {
      await (pendingRead ?? reload());
    } catch (error) {
      if (currentLifecycle === lifecycle) {
        console.error("Unable to load source mapping rules", error);
      }
    }
  }

  async function refresh(): Promise<void> {
    const currentLifecycle = lifecycle;
    await prepareListener();
    if (currentLifecycle === lifecycle) await reload();
  }

  function destroy(): void {
    lifecycle += 1;
    unlisten?.();
    unlisten = undefined;
    pendingListen = undefined;
    pendingRead = undefined;
    refreshRequested = false;
    loaded = false;
    rules = [];
  }

  return {
    initialize,
    refresh,
    destroy,
    resolveWebSourceName: (item: WebSourceItem) => resolveName(item, rules),
  };
}

export type SourceMappingStore = ReturnType<typeof createSourceMappingStore>;
const sourceMapping = createSourceMappingStore();

/** Subscribes before hydration; failed reads or registrations can be retried. */
export const ensureSourceMappingRules = sourceMapping.initialize;
/** Re-reads authoritative rules; save responses never replace the shared cache. */
export const refreshSourceMappingRules = sourceMapping.refresh;
export const destroySourceMappingRules = sourceMapping.destroy;
export const resolveWebSourceName = sourceMapping.resolveWebSourceName;
