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
    favorite: string;
    unfavorite: string;
    delete: string;
    selectItem: string;
    itemActions: string;
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
  };
}

export type LocaleMessages = Record<keyof LocaleDefinition, Record<string, string>>;
export type TranslationKey = string;
