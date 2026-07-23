export const clipboardKinds = ["text", "link", "image", "file"] as const;

export type ClipboardKind = (typeof clipboardKinds)[number];
export type ClipboardFilter = "all" | ClipboardKind | "favorite";

export interface ClipboardItem {
  id: string;
  kind: ClipboardKind;
  title: string;
  preview: string;
  sourceApp: string;
  sourceTone: "red" | "blue" | "violet" | "neutral";
  sizeLabel: string;
  detailLabel?: string;
  createdAt: number;
  favorite: boolean;
  fileName?: string;
  imageMeta?: {
    width: number;
    height: number;
  };
}

export interface RuntimeInfo {
  appVersion: string;
  operatingSystem: string;
  architecture: string;
  capabilities: {
    clipboardMonitoring: boolean;
    globalShortcut: boolean;
    quickPaste: boolean;
    systemTray: boolean;
    requiresAccessibilityPermission: boolean;
  };
}
