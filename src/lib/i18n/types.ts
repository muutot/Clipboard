export type Locale = "zh-CN" | "en";

export interface LocaleDefinition {
  app: {
    name: string;
    searchPlaceholder: string;
    clearSearch: string;
    recentRecords: string;
    noRecords: string;
    noRecordsHint: string;
    noMatchRecords: string;
    noMatchRecordsHint: string;
    activateHint: string;
    databaseLoadFailed: string;
    historyEmpty: string;
    searchHitSummary: string;
    searchFailed: string;
    activateItem: string;
    favoriteFailed: string;
    deleteFailed: string;
    browserPreview: string;
    coreConnected: string;
    shortcutHint: string;
  };
  filter: {
    all: string;
    text: string;
    link: string;
    image: string;
    file: string;
    favorite: string;
  };
  card: {
    copy: string;
    export: string;
    saveAs: string;
    favorite: string;
    unfavorite: string;
    delete: string;
    selectItem: string;
    itemActions: string;
    viewDetail: string;
    edit: string;
    pastePlain: string;
    pasteFormat: string;
  };
  time: {
    justNow: string;
  };
  toolbar: {
    clearHistory: string;
    help: string;
    pinWindow: string;
    settings: string;
  };
  status: {
    searching: string;
    recordCount: string;
  };
  storage: {
    settings: string;
    dataStorage: string;
    settingsTab: string;
    storageTab: string;
    keyboardTab: string;
    generalTab: string;
    configPath: string;
    readingConfig: string;
    systemMessage: string;
    storageUnavailable: string;
    configSectionTitle: string;
    configSectionDesc: string;
    dataDirectoryTitle: string;
    dataDirectoryDesc: string;
    custom: string;
    default: string;
    directoryPath: string;
    placeholderPath: string;
    saveDirectory: string;
    saving: string;
    restoreDefault: string;
    directoryTreeTitle: string;
    directoryTreeDesc: string;
    searchIndexTitle: string;
    searchIndexDesc: string;
    rebuildRequired: string;
    ready: string;
    rebuildIndex: string;
    rebuilding: string;
    databaseVersion: string;
    searchIndexVersion: string;
    recordCount: string;
    sqliteConnected: string;
    writeFailed: string;
    enterAbsolutePath: string;
    savedAndRestart: string;
    alreadyUsingDir: string;
    rebuildComplete: string;
  };
  keyboard: {
    settings: string;
    title: string;
    readingConfig: string;
    shortcutConfigTitle: string;
    shortcutConfigDesc: string;
    toggleWindow: string;
    quickPaste: string;
    actionCode: string;
    shortcutInput: string;
    inputPlaceholder: string;
    saveBinding: string;
    saving: string;
    saved: string;
    formatHint: string;
    chordFormat: string;
    doubleFormat: string;
    noDuplicate: string;
    browserUnavailable: string;
    keyboardUnavailable: string;
    bindingsCount: string;
  };
  capture: {
    settings: string;
    title: string;
    description: string;
    readingApps: string;
    browserUnavailable: string;
    availableApps: string;
    ignoredApps: string;
    refreshApps: string;
    searchApps: string;
    searchIgnored: string;
    noAppsFound: string;
    noIgnoredMatch: string;
    addManual: string;
    add: string;
    ignoreSelected: string;
    removeIgnored: string;
    moveOut: string;
    saved: string;
    configNote: string;
    captureUnavailable: string;
  };
  actions: {
    close: string;
    sendEmail: string;
    openUrl: string;
    callPhone: string;
    copyColor: string;
  };
  dateFilter: {
    all: string;
    today: string;
    yesterday: string;
    week: string;
    month: string;
    custom: string;
  };
  sourceApp: {
    all: string;
    placeholder: string;
  };
  search: {
    regex: string;
    regexError: string;
  };
  bulk: {
    copyN: string;
    favoriteN: string;
    deleteN: string;
    deselectAll: string;
  };
  toast: {
    copySuccess: string;
    favoriteSuccess: string;
    unfavoriteSuccess: string;
    deleteSuccess: string;
    copyFailed: string;
    bulkCopySuccess: string;
    bulkFavoriteSuccess: string;
    bulkDeleteSuccess: string;
    editSaved: string;
    plainPasteSuccess: string;
  };
  detail: {
    title: string;
    preview: string;
    details: string;
    ocr: string;
    sourceApp: string;
    contentType: string;
    copyTime: string;
    size: string;
    mimeInfo: string;
    ocrStatus: string;
    ocrText: string;
    noOcr: string;
    pending: string;
    completed: string;
    specialMarkers: string;
    fileInfo: string;
    back: string;
  };
  edit: {
    edit: string;
    editFileName: string;
    save: string;
    cancel: string;
    placeholder: string;
  };
  paste: {
    plainText: string;
    withFormat: string;
  };
  general: {
    title: string;
    language: string;
    fontSize: string;
    fontSizeSmall: string;
    fontSizeNormal: string;
    fontSizeLarge: string;
    windowTransparency: string;
    compactMode: string;
    alwaysOnTop: string;
    useSystemTitleBar: string;
    theme: string;
    themeDark: string;
  };
  statistics: {
    title: string;
    totalRecords: string;
    byType: string;
    dbSize: string;
    indexSize: string;
    ocrTasks: string;
    ocrPending: string;
    ocrCompleted: string;
    text: string;
    link: string;
    image: string;
    file: string;
  };
  captureSettings: {
    retentionPeriod: string;
    retentionPeriodDesc: string;
    maxItemCount: string;
    maxItemCountDesc: string;
    recycleBinDays: string;
    recycleBinDaysDesc: string;
    maxFileCopySize: string;
    maxFileCopySizeDesc: string;
    days: string;
    bytes: string;
  };
  export: {
    dragText: string;
    dragFile: string;
  };
}

export type LocaleMessages = Record<keyof LocaleDefinition, Record<string, string>>;
export type TranslationKey = string;
