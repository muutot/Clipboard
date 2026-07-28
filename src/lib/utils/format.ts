import { convertFileSrc } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/services/runtime";

const UNITS = ["B", "KB", "MB", "GB", "TB"];

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes === 0) return "0 B";
  const i = Math.min(UNITS.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  const value = bytes / Math.pow(1024, i);
  const precision = i === 0 ? 0 : value >= 100 ? 0 : 1;
  return `${value.toFixed(precision)} ${UNITS[i]}`;
}

export function updateSliderTrack(el: HTMLInputElement | null): void {
  if (!el) return;
  const min = Number(el.min);
  const max = Number(el.max);
  const range = max - min;
  const pct = range > 0 ? ((Number(el.value) - min) / range) * 100 : 100;
  el.style.setProperty("--slider-pct", `${pct}%`);
}

export function sliderPercentage(value: number, min: number, max: number): string {
  if (min >= max) return "100%";
  const pct = ((value - min) / (max - min)) * 100;
  return `${Math.min(100, Math.max(0, pct))}%`;
}

export function assetUrl(filePath: string | null | undefined): string | undefined {
  if (!filePath) return undefined;
  if (!isTauriRuntime()) return undefined;
  try {
    return convertFileSrc(filePath.replace(/\\/g, "/"));
  } catch {
    return undefined;
  }
}
