import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/services/runtime";

export interface StorageStatus {
  itemCount: number;
  imageCount: number;
  imageSizeBytes: number;
  fileCount: number;
  fileSizeBytes: number;
  textCount: number;
  linkCount: number;
  projectPath: string;
  configPath: string;
  keyboardConfigPath: string;
  dataDirectoryPath: string;
  usesCustomDataDirectory: boolean;
  storagePath: string;
  iconsDir: string;
  databasePath: string;
  databaseSizeBytes: number;
  filesPath: string;
  imagePath: string;
  imageCleanupEnabled: boolean;
  fileCleanupEnabled: boolean;
  searchIndexPath: string;
  searchIndexSizeBytes: number;
  searchIndexVersion: number;
  searchIndexRebuildRequired: boolean;
  diskTotalBytes: number | null;
  diskAvailableBytes: number | null;
}

export interface StorageDirectoryUpdate {
  dataDirectoryPath: string;
  storagePath: string;
  restartRequired: boolean;
}

export interface StorageConfig {
  maxFileCopySizeBytes: number;
  maxScreenshotSizeBytes: number;
  imageStoragePath: string | null;
  fileStoragePath: string | null;
}

export interface ResourceStorageUpdate {
  imageStoragePath: string;
  fileStoragePath: string;
  restartRequired: boolean;
}

export interface PerformanceMetrics {
  startup: {
    totalStartupMs: number;
    dbOpenMs: number;
    searchInitMs: number;
    migrationsMs: number;
  };
  searchLatency: {
    searchesRecorded: number;
    averageMs: number | null;
    p95Ms: number | null;
    p99Ms: number | null;
  };
  memory: {
    currentBytes: number;
    peakBytes: number;
    snapshotCount: number;
    uptimeSeconds: number;
  };
}

export interface RepairResult {
  integrityOk: boolean;
  integrityMessage: string;
  pageCount: number;
  freelistCount: number;
}

export interface SearchSyncSummary {
  processedEvents: number;
  upsertedDocuments: number;
  deletedDocuments: number;
  lastSequence: number | null;
}

export type StorageKind = "text" | "link" | "image" | "file";

export interface StorageKindStats {
  itemCount: number;
  sizeBytes: number;
}

export interface StorageKindDeleteResult {
  deletedCount: number;
  deletedSizeBytes: number;
  removedFiles: number;
  searchSync: SearchSyncSummary | null;
  warnings: string[];
}

export async function getPerformanceMetrics(): Promise<PerformanceMetrics | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<PerformanceMetrics>("get_performance_metrics");
}

export async function repairDatabase(): Promise<RepairResult | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<RepairResult>("repair_database");
}

export async function validateSearchIndex(): Promise<boolean | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<boolean>("validate_search_index");
}

export async function getStorageStatus(): Promise<StorageStatus | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<StorageStatus>("get_storage_status");
}

export async function getStorageKindStats(kind: StorageKind): Promise<StorageKindStats | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<StorageKindStats>("get_storage_kind_stats", { kind });
}

export async function permanentlyDeleteStorageKind(
  kind: StorageKind,
  expected: StorageKindStats,
): Promise<StorageKindDeleteResult> {
  if (!isTauriRuntime()) {
    throw new Error("Storage cleanup is only available in the desktop app");
  }

  return invoke<StorageKindDeleteResult>("permanently_delete_storage_kind", {
    kind,
    expected,
  });
}

export async function configureStorageDirectory(
  dataDirectory: string | null,
): Promise<StorageDirectoryUpdate> {
  if (!isTauriRuntime()) {
    throw new Error("Storage configuration is only available in the desktop app");
  }

  return invoke<StorageDirectoryUpdate>("configure_storage_directory", {
    dataDirectory,
  });
}

export async function getStorageConfig(): Promise<StorageConfig> {
  if (!isTauriRuntime()) {
    throw new Error("Storage configuration is only available in the desktop app");
  }

  return invoke<StorageConfig>("get_storage_config");
}

export async function setResourceStoragePaths(
  imageStoragePath: string | null,
  fileStoragePath: string | null,
): Promise<ResourceStorageUpdate> {
  if (!isTauriRuntime()) {
    throw new Error("Storage configuration is only available in the desktop app");
  }

  return invoke<ResourceStorageUpdate>("set_resource_storage_paths", {
    imageStoragePath,
    fileStoragePath,
  });
}

export async function rebuildSearchIndex(): Promise<SearchSyncSummary> {
  if (!isTauriRuntime()) {
    throw new Error("Search index rebuilding is only available in the desktop app");
  }

  return invoke<SearchSyncSummary>("rebuild_search_index");
}

export interface IconFileInfo {
  name: string;
  sizeBytes: number;
}

export async function listIconFiles(): Promise<IconFileInfo[]> {
  if (!isTauriRuntime()) {
    return [];
  }

  return invoke<IconFileInfo[]>("list_icon_files");
}

export async function deleteIconFiles(names: string[]): Promise<number> {
  if (!isTauriRuntime()) {
    return 0;
  }

  return invoke<number>("delete_icon_files", { names });
}
