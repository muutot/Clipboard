import { invokeTauri, invokeTauriRequired } from "$lib/services/runtime";

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
  return invokeTauri<PerformanceMetrics>("get_performance_metrics");
}

export async function repairDatabase(): Promise<RepairResult | null> {
  return invokeTauri<RepairResult>("repair_database");
}

export async function validateSearchIndex(): Promise<boolean | null> {
  return invokeTauri<boolean>("validate_search_index");
}

export async function getStorageStatus(): Promise<StorageStatus | null> {
  return invokeTauri<StorageStatus>("get_storage_status");
}

export async function getStorageKindStats(kind: StorageKind): Promise<StorageKindStats | null> {
  return invokeTauri<StorageKindStats>("get_storage_kind_stats", { kind });
}

export async function permanentlyDeleteStorageKind(
  kind: StorageKind,
  expected: StorageKindStats,
): Promise<StorageKindDeleteResult> {
  return invokeTauriRequired<StorageKindDeleteResult>(
    "permanently_delete_storage_kind",
    {
      kind,
      expected,
    },
    "Storage cleanup is only available in the desktop app",
  );
}

export async function configureStorageDirectory(
  dataDirectory: string | null,
): Promise<StorageDirectoryUpdate> {
  return invokeTauriRequired<StorageDirectoryUpdate>(
    "configure_storage_directory",
    {
      dataDirectory,
    },
    "Storage configuration is only available in the desktop app",
  );
}

export async function getStorageConfig(): Promise<StorageConfig> {
  return invokeTauriRequired<StorageConfig>(
    "get_storage_config",
    undefined,
    "Storage configuration is only available in the desktop app",
  );
}

export async function setResourceStoragePaths(
  imageStoragePath: string | null,
  fileStoragePath: string | null,
): Promise<ResourceStorageUpdate> {
  return invokeTauriRequired<ResourceStorageUpdate>(
    "set_resource_storage_paths",
    {
      imageStoragePath,
      fileStoragePath,
    },
    "Storage configuration is only available in the desktop app",
  );
}

export async function rebuildSearchIndex(): Promise<SearchSyncSummary> {
  return invokeTauriRequired<SearchSyncSummary>(
    "rebuild_search_index",
    undefined,
    "Search index rebuilding is only available in the desktop app",
  );
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
  return invokeTauri<IconCacheEntry[]>("list_icon_cache", undefined, []);
}

export async function deleteIconFiles(names: string[]): Promise<number> {
  return invokeTauri<number>("delete_icon_files", { names }, 0);
}

export async function replaceIconFile(name: string, sourcePath: string): Promise<void> {
  return invokeTauriRequired<void>(
    "replace_icon_file",
    { name, sourcePath },
    "Icon replacement is only available in the desktop app",
  );
}

export async function getExportFormats(): Promise<ExportFormatInfo[]> {
  return invokeTauri<ExportFormatInfo[]>("get_export_formats", undefined, []);
}

export async function getImportFormats(): Promise<ImportFormatInfo[]> {
  return invokeTauri<ImportFormatInfo[]>("get_import_formats", undefined, []);
}

export async function exportToFile(
  path: string,
  format: string,
  options: ExportFileOptions,
): Promise<ExportFileResult> {
  return invokeTauriRequired<ExportFileResult>(
    "export_to_file",
    {
      path,
      format,
      includeFavorites: options.includeFavorites,
      dateFromMs: options.dateFromMs ?? null,
      dateToMs: options.dateToMs ?? null,
      contentTypes: options.contentTypes,
    },
    "Export is only available in the desktop app",
  );
}

export async function importFromFile(path: string): Promise<ImportSummary> {
  return invokeTauriRequired<ImportSummary>(
    "import_from_file",
    { path },
    "Import is only available in the desktop app",
  );
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
  return invokeTauri<SyncConfig>("get_sync_config", undefined, {
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
  });
}

export async function setSyncConfig(settings: SyncConfigUpdate): Promise<void> {
  await invokeTauri<void>("set_sync_config", {
    ...settings,
    s3SecretKey: settings.s3SecretKey ?? null,
    syncPassword: settings.syncPassword ?? null,
  });
}

export async function testSyncConnection(): Promise<S3TestResult> {
  return invokeTauri<S3TestResult>("test_sync_connection", undefined, {
    success: false,
    message: "Not in desktop runtime",
    statusCode: null,
  });
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
  return invokeTauriRequired<SyncRunResult>(
    "sync_now",
    undefined,
    "Sync is only available in the desktop app",
  );
}
