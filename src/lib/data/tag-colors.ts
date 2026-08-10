export const TAG_COLOR_PRESETS: readonly string[] = [
  "#e5484d",
  "#f76b15",
  "#ffb224",
  "#46a758",
  "#3e63dd",
  "#8e4ec6",
  "#00a2c7",
  "#5c7cfa",
  "#d6409f",
  "#12a594",
  "#ad5700",
  "#6b7280",
  "#84cc16",
  "#d946ef",
  "#f8fafc",
];

export const TAG_COLOR_FALLBACK = "#5c7cfa";

export function isTagColorPreset(color: string): boolean {
  return TAG_COLOR_PRESETS.includes(color);
}

export function isValidTagColor(color: string): boolean {
  return /^#[0-9a-fA-F]{6}$/.test(color);
}
