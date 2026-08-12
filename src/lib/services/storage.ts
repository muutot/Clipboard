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

export interface ExportFormatInfo {
  id: string;
  label: string;
  extension: string;
}

export interface ImportFormatInfo {
  id: string;
  label: string;
  extension: string;
}

export interface ExportFileOptions {
  includeFavorites: boolean;
  dateFromMs?: number | null;
  dateToMs?: number | null;
  contentTypes: string[];
}

export interface ExportFileResult {
  path: string;
  format: string;
  byteCount: number;
}

export interface ImportSummary {
  importedCount: number;
  skippedCount: number;
  errors: string[];
  pendingTruncation: number;
  maxItems: number;
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

export interface IconCacheEntry {
  appName: string | null;
  displayName: string;
  iconName: string | null;
  contentHash: string | null;
  targetIconName: string;
  sizeBytes: number;
  firstChar: string;
}

export async function listIconCache(): Promise<IconCacheEntry[]> {
  if (!isTauriRuntime()) {
    return [];
  }

  return invoke<IconCacheEntry[]>("list_icon_cache");
}

export async function deleteIconFiles(names: string[]): Promise<number> {
  if (!isTauriRuntime()) {
    return 0;
  }

  return invoke<number>("delete_icon_files", { names });
}

export async function replaceIconFile(name: string, sourcePath: string): Promise<void> {
  if (!isTauriRuntime()) {
    throw new Error("Icon replacement is only available in the desktop app");
  }

  return invoke<void>("replace_icon_file", { name, sourcePath });
}

export async function getExportFormats(): Promise<ExportFormatInfo[]> {
  if (!isTauriRuntime()) {
    return [];
  }

  return invoke<ExportFormatInfo[]>("get_export_formats");
}

export async function getImportFormats(): Promise<ImportFormatInfo[]> {
  if (!isTauriRuntime()) {
    return [];
  }

  return invoke<ImportFormatInfo[]>("get_import_formats");
}

export async function exportToFile(
  path: string,
  format: string,
  options: ExportFileOptions,
): Promise<ExportFileResult> {
  if (!isTauriRuntime()) {
    throw new Error("Export is only available in the desktop app");
  }

  return invoke<ExportFileResult>("export_to_file", {
    path,
    format,
    includeFavorites: options.includeFavorites,
    dateFromMs: options.dateFromMs ?? null,
    dateToMs: options.dateToMs ?? null,
    contentTypes: options.contentTypes,
  });
}

export async function importFromFile(path: string): Promise<ImportSummary> {
  if (!isTauriRuntime()) {
    throw new Error("Import is only available in the desktop app");
  }

  return invoke<ImportSummary>("import_from_file", { path });
}

export interface SyncConfig {
  provider: "off" | "s3";
  endpoint: string | null;
  remotePath: string | null;
  s3Region: string | null;
  s3Bucket: string | null;
  s3AccessKey: string | null;
  hasS3SecretKey: boolean;
  hasSyncPassword: boolean;
  lastSyncMs: number | null;
  lastSyncStatus: string | null;
  pendingEntries: number;
  autoSync: boolean;
  autoSyncIntervalSecs: number;
  segmentMaxEntries: number;
  maxSyncImageBytes: number;
  maxSyncFileBytes: number;
}

export interface S3TestResult {
  success: boolean;
  message: string;
  statusCode: number | null;
}

export interface SyncConfigUpdate {
  provider: "off" | "s3";
  endpoint: string | null;
  remotePath: string | null;
  autoSync: boolean;
  autoSyncIntervalSecs: number;
  segmentMaxEntries: number;
  maxSyncImageBytes: number;
  maxSyncFileBytes: number;
  s3Region: string | null;
  s3Bucket: string | null;
  s3AccessKey: string | null;
  s3SecretKey?: string | null;
  syncPassword?: string | null;
}

export async function getSyncConfig(): Promise<SyncConfig> {
  if (!isTauriRuntime()) {
    return {
      provider: "off",
      endpoint: null,
      remotePath: "clipboard-sync",
      lastSyncMs: null,
      lastSyncStatus: null,
      pendingEntries: 0,
      autoSync: false,
      autoSyncIntervalSecs: 300,
      segmentMaxEntries: 512,
      maxSyncImageBytes: 5242880,
      maxSyncFileBytes: 10485760,
      s3Region: "us-east-1",
      s3Bucket: null,
      s3AccessKey: null,
      hasS3SecretKey: false,
      hasSyncPassword: false,
    };
  }
  return invoke<SyncConfig>("get_sync_config");
}

export async function setSyncConfig(settings: SyncConfigUpdate): Promise<void> {
  if (!isTauriRuntime()) return;
  return invoke<void>("set_sync_config", {
    ...settings,
    s3SecretKey: settings.s3SecretKey ?? null,
    syncPassword: settings.syncPassword ?? null,
  });
}

export async function testSyncConnection(): Promise<S3TestResult> {
  if (!isTauriRuntime()) {
    return { success: false, message: "Not in desktop runtime", statusCode: null };
  }
  return invoke<S3TestResult>("test_sync_connection");
}

export interface SyncRunResult {
  uploadedEntries: number;
  downloadedEntries: number;
  appliedEntries: number;
  failedPeers: number;
  uploadedResources: number;
  downloadedResources: number;
  deletedRemoteObjects: number;
  bytesUploaded: number;
  bytesDownloaded: number;
}

export async function runSync(): Promise<SyncRunResult> {
  if (!isTauriRuntime()) throw new Error("Sync is only available in the desktop app");
  return invoke<SyncRunResult>("sync_now");
}
