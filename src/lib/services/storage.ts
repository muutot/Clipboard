import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/services/runtime";

export interface StorageStatus {
  schemaVersion: number;
  itemCount: number;
  projectPath: string;
  configPath: string;
  keyboardConfigPath: string;
  dataDirectoryPath: string;
  usesCustomDataDirectory: boolean;
  storagePath: string;
  databasePath: string;
  filesPath: string;
  imagePath: string;
  searchIndexPath: string;
  searchIndexVersion: number;
  searchIndexRebuildRequired: boolean;
}

export interface StorageDirectoryUpdate {
  dataDirectoryPath: string;
  storagePath: string;
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

export async function rebuildSearchIndex(): Promise<SearchSyncSummary> {
  if (!isTauriRuntime()) {
    throw new Error("Search index rebuilding is only available in the desktop app");
  }

  return invoke<SearchSyncSummary>("rebuild_search_index");
}
