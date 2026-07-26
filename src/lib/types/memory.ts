/** A best-effort snapshot of the desktop process group and host memory. */
export interface MemoryDiagnostics {
  sampledAtMs: number;
  currentProcess: MemoryProcess;
  processGroup: MemoryProcessGroup;
  system: SystemMemory;
  ocr: OcrMemoryDiagnostics;
}

export interface MemoryProcess {
  pid: number;
  parentPid: number | null;
  name: string;
  role?: string | null;
  /** Resident memory including shared pages. */
  workingSetBytes: number | null;
  /** Private committed bytes when the platform exposes them. */
  privateBytes: number | null;
  /** Private resident working set when the platform exposes or approximates it. */
  privateWorkingSetBytes: number | null;
  /** Private committed bytes on Windows; virtual address size on Linux/macOS. */
  virtualBytes: number | null;
}

export interface MemoryProcessGroup {
  workingSetBytes: number;
  privateBytes: number;
  virtualBytes: number;
  processes: MemoryProcess[];
}

export interface SystemMemory {
  totalBytes: number | null;
  availableBytes: number | null;
}

export interface OcrMemoryDiagnostics {
  engine: string;
  modelVariant: string;
  modelBytes: number;
  modelFileCount: number;
  modelDirectory: string;
  /** True when PP-OCR is selected and all configured model files are installed. */
  loaded: boolean;
  installedVariants: string[];
}
