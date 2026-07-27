<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { IconName } from "$lib/components/AppIcon.svelte";
  import {
    configureKeyboardShortcuts,
    getKeyboardConfig,
    type KeyboardConfig,
  } from "$lib/services/keyboard";
  import { messages, resolvePath } from "$lib/i18n";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
    category?: "item" | "quick" | "system";
    resetToken?: number;
  }

  let { onclose, showHeader = true, category = "item", resetToken = 0 }: Props = $props();
  let config = $state<KeyboardConfig | null>(null);
  let loading = $state(true);
  let feedback = $state("");
  let feedbackSuccess = $state(false);
  let recordingAction = $state("");

  interface SystemAction {
    id: string;
    labelKey?: string;
    description: string;
    icon: IconName;
    defaults: string[];
    cat: typeof category;
    system?: boolean;
  }

  const SYSTEM_ACTIONS: SystemAction[] = [
    { id: "copyItem", labelKey: "keyboard.copyItem", description: "复制当前选中的条目到剪贴板", icon: "copy", defaults: ["Ctrl+C"], cat: "item" },
    { id: "deleteItem", labelKey: "keyboard.deleteItem", description: "删除当前选中的条目", icon: "trash", defaults: ["Ctrl+D"], cat: "item" },
    { id: "favoriteItem", labelKey: "keyboard.favoriteItem", description: "收藏或取消收藏当前条目", icon: "star", defaults: ["Ctrl+F"], cat: "item" },
    { id: "editItem", labelKey: "keyboard.editItem", description: "编辑当前条目的标题", icon: "edit", defaults: ["Ctrl+E"], cat: "item" },
    { id: "openDetail", labelKey: "keyboard.openDetail", description: "预览当前条目的详情", icon: "eye", defaults: ["Ctrl+E"], cat: "item" },
    { id: "downloadItem", labelKey: "keyboard.downloadItem", description: "保存图片或文件到本地", icon: "download", defaults: ["Ctrl+S"], cat: "item" },
    { id: "selectAll", labelKey: "keyboard.selectAll", description: "全选列表中的所有条目", icon: "check", defaults: ["Ctrl+A"], cat: "item" },
    { id: "quickPaste", labelKey: "keyboard.quickPaste", description: "将当前条目快速粘贴到上一个活跃窗口", icon: "clipboard", defaults: [], cat: "item" },
    { id: "quickCopy1", description: "快速复制列表第 1 条", icon: "clipboard", defaults: ["Ctrl+1"], cat: "quick" },
    { id: "quickCopy2", description: "快速复制列表第 2 条", icon: "clipboard", defaults: ["Ctrl+2"], cat: "quick" },
    { id: "quickCopy3", description: "快速复制列表第 3 条", icon: "clipboard", defaults: ["Ctrl+3"], cat: "quick" },
    { id: "quickCopy4", description: "快速复制列表第 4 条", icon: "clipboard", defaults: ["Ctrl+4"], cat: "quick" },
    { id: "quickCopy5", description: "快速复制列表第 5 条", icon: "clipboard", defaults: ["Ctrl+5"], cat: "quick" },
    { id: "quickCopy6", description: "快速复制列表第 6 条", icon: "clipboard", defaults: ["Ctrl+6"], cat: "quick" },
    { id: "quickCopy7", description: "快速复制列表第 7 条", icon: "clipboard", defaults: ["Ctrl+7"], cat: "quick" },
    { id: "quickCopy8", description: "快速复制列表第 8 条", icon: "clipboard", defaults: ["Ctrl+8"], cat: "quick" },
    { id: "quickCopy9", description: "快速复制列表第 9 条", icon: "clipboard", defaults: ["Ctrl+9"], cat: "quick" },
    { id: "toggleWindow", labelKey: "keyboard.toggleWindow", description: "唤起或隐藏主窗口（系统全局热键）", icon: "eye", defaults: ["Alt+V"], cat: "system", system: true },
  ];

  const categoryActions = $derived.by(() => {
    return SYSTEM_ACTIONS.filter((a) => a.cat === category);
  });

  function actionLabel(action: SystemAction): string {
    if (action.labelKey) {
      const label = _t(action.labelKey as keyof typeof $messages);
      if (label) return label;
    }
    const m = action.id.match(/^quickCopy(\d+)$/);
    if (m) return `快速复制 #${m[1]}`;
    return action.id;
  }

  function bindingsFor(action: string): string[] {
    if (!config) return [];
    return config.shortcuts[action] ?? [];
  }

  onMount(() => {
    void loadConfig();
  });

  $effect(() => {
    resetToken;
    if (resetToken > 0) void loadConfig();
  });

  async function loadConfig() {
    loading = true;
    feedback = "";

    try {
      config = await getKeyboardConfig();
      if (!config) feedback = _t("keyboard.browserUnavailable");
    } catch (error) {
      feedback = error instanceof Error ? error.message : String(error);
    } finally {
      loading = false;
    }
  }

  function showFeedback(msg: string, success: boolean) {
    feedback = msg;
    feedbackSuccess = success;
    setTimeout(() => (feedback = ""), 2000);
  }

  async function addBinding(action: string, shortcut: string) {
    if (!config) return;
    const current = config.shortcuts[action] ?? [];
    try {
      const normalized = await configureKeyboardShortcuts(action, [...current, shortcut]);
      config = { ...config, shortcuts: { ...config.shortcuts, [action]: normalized } };
    } catch (error) {
      showFeedback(error instanceof Error ? error.message : String(error), false);
    }
  }

  async function removeBinding(action: string, shortcut: string) {
    if (!config) return;
    const current = config.shortcuts[action] ?? [];
    try {
      const normalized = await configureKeyboardShortcuts(
        action,
        current.filter((s) => s !== shortcut),
      );
      config = { ...config, shortcuts: { ...config.shortcuts, [action]: normalized } };
    } catch (error) {
      showFeedback(error instanceof Error ? error.message : String(error), false);
    }
  }

  function startRecording(action: string) {
    recordingAction = action;
    window.addEventListener("keydown", onRecordingKey);
    setTimeout(() => stopRecording(), 3000);
  }

  function stopRecording() {
    recordingAction = "";
    window.removeEventListener("keydown", onRecordingKey);
  }

  function onRecordingKey(event: KeyboardEvent) {
    event.preventDefault();
    event.stopPropagation();

    if (event.key === "Escape") { stopRecording(); return; }

    const modKeys = ["Control", "Alt", "Shift", "Meta"];
    if (modKeys.includes(event.key)) return;

    const pressed: string[] = [];
    if (event.ctrlKey) pressed.push("Ctrl");
    if (event.altKey) pressed.push("Alt");
    if (event.shiftKey) pressed.push("Shift");
    if (event.metaKey) pressed.push("Meta");

    const ignored = ["AltGraph", "NumLock", "ScrollLock", "PrintScreen"];
    if (!ignored.includes(event.key)) {
      pressed.push(event.key === " " ? "Space" : event.key.length === 1 ? event.key.toUpperCase() : event.key);
    }

    if (pressed.length === 0) return;
    const action = recordingAction;
    stopRecording();
    if (action) void addBinding(action, pressed.join("+"));
  }

  onDestroy(() => {
    window.removeEventListener("keydown", onRecordingKey);
  });
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("keyboard.settings")}</span>
      <h2>{_t("keyboard.title")}</h2>
      <p>{_t("keyboard.description")}</p>
    </div>
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}>×</button>
  </header>
{/if}

{#if loading}
  <div class="settings-state">{_t("keyboard.readingConfig")}</div>
{:else if config}
  <div class="settings-scroll">
    {#each categoryActions as action}
      <section class="setting-card toggle-card">
        <div class="setting-heading">
          <span class="setting-icon"><AppIcon name={action.icon} size={17} /></span>
          <div>
            <strong>
              {actionLabel(action)}
              {#if action.system}
                <span class="system-badge">系统</span>
              {/if}
            </strong>
            <p>{action.description}</p>
          </div>
        </div>
        <div class="shortcut-bindings">
          {#if bindingsFor(action.id).length > 0}
            {#each bindingsFor(action.id) as shortcut}
              <div class="binding-chip">
                <kbd>{shortcut}</kbd>
                <button type="button" class="binding-chip-close" onclick={() => removeBinding(action.id, shortcut)}>&minus;</button>
              </div>
            {/each}
          {:else if config && !(action.id in config.shortcuts) && action.defaults.length > 0}
            {#each action.defaults as shortcut}
              <div class="binding-chip default">
                <kbd>{shortcut}</kbd>
                <button type="button" class="binding-chip-close" onclick={() => removeBinding(action.id, shortcut)}>&minus;</button>
              </div>
            {/each}
          {:else}
            <span class="binding-disabled">未绑定</span>
          {/if}

          {#if recordingAction === action.id}
            <div class="binding-chip recording">
              <kbd>按下快捷键…</kbd>
              <button type="button" class="binding-chip-close" onclick={stopRecording}>&times;</button>
            </div>
          {:else}
            <button type="button" class="binding-add" onclick={() => startRecording(action.id)}>+</button>
          {/if}
        </div>
      </section>
    {/each}

    <p class="auto-save-note">{_t("keyboard.autoSaveNote")}</p>
  </div>
{:else}
  <div class="settings-state">{feedback || _t("keyboard.keyboardUnavailable")}</div>
{/if}

{#if feedback && config}
  <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
{/if}

<style>
  .system-badge {
    display: inline-block;
    padding: 1px 5px;
    border: 1px solid var(--selection-color);
    border-radius: 4px;
    color: var(--selection-color);
    font-size: 9px;
    font-weight: 500;
    vertical-align: middle;
    margin-left: 4px;
  }

  .shortcut-bindings {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .binding-chip {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 30px;
    padding: 0 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    background: var(--input-bg);
    box-sizing: border-box;
  }

  .binding-chip kbd {
    font: 11px "Cascadia Code", Consolas, monospace;
    color: var(--text-primary);
  }

  .binding-chip.recording {
    border-color: var(--selection-color);
    animation: pulse-recording 1s ease-in-out infinite;
  }

  @keyframes pulse-recording {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }

  .binding-add {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 1px dashed var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-muted);
    background: transparent;
    font-size: 17px;
    cursor: pointer;
    transition: color 100ms ease, border-color 100ms ease;
  }

  .binding-add:hover {
    color: var(--text-secondary);
    border-color: var(--text-muted);
  }

  .binding-disabled {
    color: var(--text-faint);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
    font-style: italic;
  }

  .binding-chip-close {
    position: absolute;
    top: -7px;
    right: -7px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 50%;
    font-size: 10px;
    line-height: 1;
    color: var(--text-muted);
    background: var(--card-bg);
    cursor: pointer;
    opacity: 0;
    transition: opacity 100ms ease;
  }

  .binding-chip:hover .binding-chip-close {
    opacity: 1;
  }

  .binding-chip-close:hover {
    color: var(--danger-color);
    border-color: var(--danger-color);
    background: color-mix(in srgb, var(--danger-color) 12%, transparent);
  }

  .settings-state {
    display: grid;
    flex: 1;
    place-items: center;
    color: var(--text-muted);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
  }
</style>
