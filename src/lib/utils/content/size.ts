export type SizeUnit = "byte" | "KB" | "MB" | "GB";

export const SIZE_UNIT_MULTIPLIERS: Record<SizeUnit, number> = {
  byte: 1,
  KB: 1024,
  MB: 1048576,
  GB: 1073741824,
};

export const SIZE_UNIT_OPTIONS: { value: SizeUnit; label: string }[] = [
  { value: "byte", label: "B" },
  { value: "KB", label: "KB" },
  { value: "MB", label: "MB" },
  { value: "GB", label: "GB" },
];

/** Unknown or empty units fall back to bytes so persisted values never yield NaN. */
function unitFactor(unit: SizeUnit | string): number {
  return SIZE_UNIT_MULTIPLIERS[unit as SizeUnit] ?? 1;
}

export function toDisplaySize(bytes: number, unit: SizeUnit | string): number {
  return Math.round(bytes / unitFactor(unit));
}

export function fromDisplaySize(value: number, unit: SizeUnit | string): number {
  return Math.round(value * unitFactor(unit));
}
