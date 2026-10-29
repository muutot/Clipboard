/**
 * Known domain/brand names are intentionally non-localized. Structured pairs
 * preserve spaces in names; user rules override these built-in defaults.
 */
export const BUILTIN_DOMAIN_NAMES: ReadonlyMap<string, string> = new Map<string, string>([
  ["github.com", "GitHub"],
  ["gitlab.com", "GitLab"],
  ["stackoverflow.com", "Stack Overflow"],
  ["mozilla.org", "MDN"],
  ["microsoft.com", "Microsoft"],
  ["wikipedia.org", "Wikipedia"],
  ["youtube.com", "YouTube"],
  ["bilibili.com", "Bilibili"],
  ["zhihu.com", "知乎"],
  ["juejin.cn", "掘金"],
  ["csdn.net", "CSDN"],
  ["v2ex.com", "V2EX"],
  ["reddit.com", "Reddit"],
  ["x.com", "X"],
  ["npmjs.com", "npm"],
  ["medium.com", "Medium"],
  ["notion.so", "Notion"],
]);
