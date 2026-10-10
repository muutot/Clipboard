// Byte-size unit conversion shared by the storage limits and sync panels.
// Display values are integers in the selected unit; canonical values are
// bytes. Unknown units fall back to a 1× multiplier (bytes).

export type ByteUnit = "byte" | "KB" | "MB" | "GB";

export const BYTE_UNIT_MULTIPLIERS: Record<string, number> = {
  byte: 1,
  KB: 1024,
  MB: 1024 * 1024,
  GB: 1024 * 1024 * 1024,
};

export function toDisplaySize(bytes: number, unit: string): number {
  return Math.round(bytes / (BYTE_UNIT_MULTIPLIERS[unit] || 1));
}

export function fromDisplaySize(value: number, unit: string): number {
  return Math.round(value * (BYTE_UNIT_MULTIPLIERS[unit] || 1));
}
