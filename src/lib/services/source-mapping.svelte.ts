// Web-source display-name resolution.
//
// Browser captures carry a `web` metadata blob (page title, URL, domain). The
// card/detail views show a short display name instead of the raw page title,
// resolved as: a user mapping rule (domain or title keyword) → a built-in
// well-known-domain name → a truncated page title → the source app name.
//
// Rules live in `conf/conf.json` (`sourceMapping.rules`, edited in the
// Capture → Web source settings panel). They are loaded once per session into
// this module store; the settings panel refreshes the cache after saving.

import { getSourceMappingRules, type SourceMappingRule } from "$lib/services/clipboard";
import { isTauriRuntime } from "$lib/services/runtime";
import type { WebSourceMetadata } from "$lib/types/clipboard";

/** Cap for raw page titles shown when no rule matches. */
const WEB_TITLE_MAX_LENGTH = 24;

/**
 * Well-known domains with a shorter display name. Keys are matched on the
 * stored domain or any of its subdomains; user rules always win over these.
 */
const BUILTIN_DOMAIN_NAMES: Record<string, string> = {
  "github.com": "GitHub",
  "gitlab.com": "GitLab",
  "stackoverflow.com": "Stack Overflow",
  "mozilla.org": "MDN",
  "microsoft.com": "Microsoft",
  "wikipedia.org": "Wikipedia",
  "youtube.com": "YouTube",
  "bilibili.com": "Bilibili",
  "zhihu.com": "知乎",
  "juejin.cn": "掘金",
  "csdn.net": "CSDN",
  "v2ex.com": "V2EX",
  "reddit.com": "Reddit",
  "x.com": "X",
  "npmjs.com": "npm",
  "medium.com": "Medium",
  "notion.so": "Notion",
};

let rules = $state<SourceMappingRule[]>([]);
let rulesLoaded = false;

if (isTauriRuntime()) void ensureSourceMappingRules();

/** Loads the persisted rules once; later calls are no-ops. */
export async function ensureSourceMappingRules(): Promise<void> {
  if (rulesLoaded) return;
  rulesLoaded = true;
  try {
    rules = (await getSourceMappingRules()) ?? [];
  } catch (error) {
    console.error("Unable to load source mapping rules", error);
  }
}

/** Replaces the cached rules; used after the settings panel saves. */
export function refreshSourceMappingRules(value: SourceMappingRule[]): void {
  rules = value;
  rulesLoaded = true;
}

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

/**
 * Short display name for a capture: the first matching user rule, else a
 * built-in domain name, else the truncated page title, else the source app.
 */
export function resolveWebSourceName(item: {
  sourceApp: string;
  webSource?: WebSourceMetadata;
}): string {
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
    for (const [domain, name] of Object.entries(BUILTIN_DOMAIN_NAMES)) {
      if (matchesDomain(web.domain, domain)) return name;
    }
  }
  if (web.title) return truncateTitle(web.title);
  return item.sourceApp;
}
