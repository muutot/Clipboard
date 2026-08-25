import type { IconName } from "$lib/types/clipboard";

export type SizeUnit = "byte" | "KB" | "MB" | "GB";

export interface NumberEntryConfig {
  type: "number";
  id?: string;
  icon?: IconName;
  label: string;
  desc?: string;
  get: () => number;
  set: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  suffix?: string;
  variant?: "row" | "card";
  onchange?: () => void;
  onblur?: () => void;
}

export interface SizeEntryConfig {
  type: "size";
  id?: string;
  icon?: IconName;
  label: string;
  desc?: string;
  get: () => number;
  set: (value: number) => void;
  getUnit: () => SizeUnit;
  setUnit: (unit: SizeUnit) => void;
  min?: number;
  oninput?: () => void;
  onchange?: () => void;
}

export interface SliderEntryConfig {
  type: "slider";
  id?: string;
  icon?: IconName;
  label: string;
  desc?: string;
  get: () => number;
  set: (value: number) => void;
  min: number | (() => number);
  max: number | (() => number);
  step?: number;
  suffix?: string;
  oninput?: () => void;
  onchange?: () => void;
  /** Optional low/high scale labels rendered under the track. */
  scale?: [string, string];
  /** Optional display formatter for the current-value label. */
  display?: (value: number) => string;
}

export interface ToggleEntryConfig {
  type: "toggle";
  id?: string;
  icon?: IconName;
  label: string;
  desc?: string;
  get: () => boolean;
  set: (checked: boolean) => void;
  disabled?: () => boolean;
  onchange?: () => void;
  variant?: "row" | "card";
  ariaLabel?: string;
}

export interface SelectEntryConfig {
  type: "select";
  id?: string;
  icon?: IconName;
  label: string;
  desc?: string;
  get: () => string | number;
  set: (value: string | number) => void;
  options: { value: string | number; label: string; disabled?: boolean }[];
  ariaLabel?: string;
  variant?: "row" | "card";
  disabled?: boolean;
  onchange?: () => void;
}

export interface TextEntryConfig {
  type: "text";
  id?: string;
  icon?: IconName;
  label: string;
  desc?: string;
  get: () => string;
  set: (value: string) => void;
  inputType?: "text" | "url" | "password" | "email";
  placeholder?: string;
  maxlength?: number;
  variant?: "row" | "card";
  actionLabel?: string | (() => string);
  actionVisible?: () => boolean;
  onaction?: () => void;
  onblur?: () => void;
  onchange?: () => void;
}

export interface CustomEntryConfig {
  type: "custom";
  id?: string;
  icon?: IconName;
  label: string;
  desc?: string;
  variant?: "toggle" | "column";
  actionLabel?: string | (() => string);
  actionVisible?: () => boolean;
  onaction?: () => void;
}

export interface HeadingEntryConfig {
  type: "heading";
  id?: string;
  icon?: IconName;
  label: string;
  desc?: string;
  actionLabel?: string;
  actionDisabled?: boolean;
  onaction?: () => void;
}

export type SettingEntryConfig =
  | NumberEntryConfig
  | SizeEntryConfig
  | SliderEntryConfig
  | ToggleEntryConfig
  | SelectEntryConfig
  | TextEntryConfig
  | HeadingEntryConfig
  | CustomEntryConfig;
