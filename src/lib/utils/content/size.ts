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

export function toDisplaySize(bytes: number, unit: SizeUnit): number {
  return Math.round(bytes / SIZE_UNIT_MULTIPLIERS[unit]);
}

export function fromDisplaySize(value: number, unit: SizeUnit): number {
  return Math.round(value * SIZE_UNIT_MULTIPLIERS[unit]);
}
