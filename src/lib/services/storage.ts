import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/services/runtime";

export interface StorageStatus {
  schemaVersion: number;
  itemCount: number;
  projectPath: string;
  configPath: string;
  dataDirectoryPath: string;
  usesCustomDataDirectory: boolean;
  storagePath: string;
  databasePath: string;
  filesPath: string;
  imagePath: string;
  searchIndexPath: string;
}

export interface StorageDirectoryUpdate {
  dataDirectoryPath: string;
  storagePath: string;
  restartRequired: boolean;
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
