<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import {
    configureKeyboardShortcuts,
    getKeyboardConfig,
    type KeyboardConfig,
  } from "$lib/services/keyboard";
  import { messages, resolvePath } from "$lib/i18n";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    configPath?: string;
    onclose: () => void;
    showHeader?: boolean;
  }

  let { configPath = "conf/keyboard.json", onclose, showHeader = true }: Props = $props();
  let config = $state<KeyboardConfig | null>(null);
  let drafts = $state<Record<string, string>>({});
  let loading = $state(true);
  let savingAction = $state("");
  let feedback = $state("");
  let feedbackSuccess = $state(false);
  let categoryTab = $state("item");

  type Category = "window" | "item" | "navigation" | "search" | "other";

  const categoryTabs: { id: Category; label: string }[] = [
    { id: "item", label: "条目操作" },
    { id: "navigation", label: "列表导航" },
    { id: "window", label: "窗口控制" },
    { id: "search", label: "搜索" },
    { id: "other", label: "其他" },
  ];

  function classifyAction(action: string): Category {
    const lower = action.toLowerCase();
    if (/window|toggle|show|hide/.test(lower)) return "window";
    if (/copy|paste|delete|favorite|edit|detail|download|save|pin/.test(lower)) return "item";
    if (/select|next|prev|up|down|left|right|tab|focus|navigate|number|quick/.test(lower)) return "navigation";
    if (/search|find|clear/.test(lower)) return "search";
    return "other";
  }

  const shortcutsByCategory = $derived.by(() => {
    if (!config) return [];
    return Object.entries(config.shortcuts)
      .filter(([a]) => a !== "toggleWindow")
      .filter(([a]) => classifyAction(a) === categoryTab);
  });

  const actionLabels: Record<string, string> = $derived({
    toggleWindow: _t("keyboard.toggleWindow") || "唤起窗口",
    copyItem: _t("keyboard.copyItem") || "复制条目",
    deleteItem: _t("keyboard.deleteItem") || "删除条目",
    favoriteItem: _t("keyboard.favoriteItem") || "收藏条目",
    editItem: _t("keyboard.editItem") || "编辑条目",
    selectAll: _t("keyboard.selectAll") || "全选条目",
    quickPaste: _t("keyboard.quickPaste"),
  });

  onMount(() => {
    void loadConfig();
  });

  async function loadConfig() {
    loading = true;
    feedback = "";
    feedbackSuccess = false;

    try {
      config = await getKeyboardConfig();
      if (!config) {
        feedback = _t("keyboard.browserUnavailable");
        return;
      }
      drafts = Object.fromEntries(
        Object.entries(config.shortcuts).map(([action, shortcuts]) => [
          action,
          shortcuts.join(", "),
        ]),
      );
    } catch (error) {
      console.error("Unable to load keyboard configuration", error);
      feedback = error instanceof Error ? error.message : String(error);
    } finally {
      loading = false;
    }
  }

  async function saveAction(action: string) {
    savingAction = action;
    feedback = "";
    feedbackSuccess = false;

    try {
      const shortcuts = (drafts[action] ?? "")
        .split(/[,;\n]+/)
        .map((shortcut) => shortcut.trim())
        .filter(Boolean);
      const normalized = await configureKeyboardShortcuts(action, shortcuts);
      drafts[action] = normalized.join(", ");
      if (config) config.shortcuts[action] = normalized;
      feedback = _t("keyboard.saved", { count: normalized.length });
      feedbackSuccess = true;
    } catch (error) {
      console.error("Unable to save keyboard shortcuts", error);
      feedback = error instanceof Error ? error.message : String(error);
    } finally {
      savingAction = "";
    }
  }

  function actionLabel(action: string): string {
    return actionLabels[action] ?? action;
  }
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("keyboard.settings")}</span>
      <h2>{_t("keyboard.title")}</h2>
      <p>{_t("keyboard.description")}</p>
    </div>
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
    >
  </header>
{/if}

{#if loading}
  <div class="settings-state">{_t("keyboard.readingConfig")}</div>
{:else if config}
  <div class="settings-scroll">
    <section class="setting-card setting-card-row">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="keyboard" size={17} /></span>
        <div>
          <strong>{_t("keyboard.shortcutConfigTitle")}</strong>
          <p>{_t("keyboard.shortcutConfigDesc")}</p>
        </div>
      </div>
      <button
        type="button"
        class="open-btn"
        onclick={() => invoke("open_external_url", { url: configPath })}
      >
        <AppIcon name="file" size={14} /> 打开文件
      </button>
    </section>

    <nav class="settings-subnav" aria-label="快捷键分类">
      {#each categoryTabs as tab}
        <button
          type="button"
          class:active={categoryTab === tab.id}
          aria-current={categoryTab === tab.id ? "page" : undefined}
          onclick={() => (categoryTab = tab.id)}
        >
          {tab.label}
        </button>
      {/each}
    </nav>

    {#each shortcutsByCategory as [action]}
      <section class="setting-card">
        <div class="setting-heading split-heading">
          <div>
            <strong>{actionLabel(action)}</strong>
            <p><code>{action}</code> · {_t("keyboard.actionCode")}</p>
          </div>
          <span class="binding-count"
            >{_t("keyboard.bindingsCount", { count: config.shortcuts[action].length })}</span
          >
        </div>
        <label for={`shortcut-${action}`}>{_t("keyboard.shortcutInput")}</label>
        <input
          id={`shortcut-${action}`}
          value={drafts[action] ?? ""}
          oninput={(event) => (drafts[action] = event.currentTarget.value)}
          autocomplete="off"
          spellcheck="false"
          placeholder={_t("keyboard.inputPlaceholder")}
        />
        <div class="setting-actions">
          <button
            class="primary"
            type="button"
            disabled={savingAction !== ""}
            onclick={() => saveAction(action)}
          >
            {savingAction === action ? _t("keyboard.saving") : _t("keyboard.saveBinding")}
          </button>
        </div>
      </section>
    {/each}

    <section class="shortcut-reference">
      <strong>当前快捷键参考</strong>
      <div class="ref-grid">
        <div class="ref-row"><kbd>↑</kbd><kbd>↓</kbd><span>选择条目</span></div>
        <div class="ref-row"><kbd>←</kbd><kbd>→</kbd><kbd>Tab</kbd><span>切换分类</span></div>
        <div class="ref-row"><kbd>Enter</kbd><span>激活</span></div>
        <div class="ref-row"><kbd>ESC</kbd><span>隐藏窗口</span></div>
        <div class="ref-row">
          <kbd>Ctrl</kbd>+<kbd>C</kbd><kbd>D</kbd><kbd>F</kbd><kbd>E</kbd><span
            >复制 / 删除 / 收藏 / 编辑</span
          >
        </div>
        <div class="ref-row"><kbd>Ctrl</kbd>+<kbd>数字</kbd><span>快速复制第 N 条</span></div>
        <div class="ref-row"><kbd>Alt</kbd>+<kbd>V</kbd><span>唤起窗口</span></div>
        <div class="ref-row"><kbd>Backspace</kbd>×2<span>清空搜索</span></div>
      </div>
    </section>

    <p class="auto-save-note">修改即时生效，无需手动保存</p>
  </div>
{:else}
  <div class="settings-state">{feedback || _t("keyboard.keyboardUnavailable")}</div>
{/if}

{#if feedback && config}
  <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
{/if}

<style>
  .split-heading {
    justify-content: space-between;
  }

  .setting-heading code {
    color: var(--text-muted);
  }

  .binding-count {
    flex: 0 0 auto;
    padding: 3px 7px;
    border: 1px solid var(--border-color);
    border-radius: 999px;
    color: var(--text-muted);
    font-size: var(--settings-note-size, var(--font-size-tiny, 10px));
  }

  label {
    display: block;
    margin: 12px 0 6px;
    color: var(--text-muted);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
  }

  input {
    width: 100%;
    box-sizing: border-box;
    padding: 9px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    outline: none;
    color: var(--text-primary);
    background: var(--input-bg);
    font-family: "Cascadia Code", Consolas, monospace;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
  }

  input:focus {
    border-color: var(--text-faint);
  }

  .setting-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 9px;
  }

  .setting-actions button {
    padding: 7px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-secondary);
    background: var(--card-bg);
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .setting-actions button:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  button:disabled {
    cursor: wait;
    opacity: 0.55;
  }

  .settings-state {
    display: grid;
    flex: 1;
    place-items: center;
    color: var(--text-muted);
    font-size: var(--settings-description-size, var(--font-size-secondary, 11px));
  }

  .shortcut-reference {
    padding: 10px 13px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--settings-card-radius, 9px);
    background: var(--card-bg);
  }

  .shortcut-reference strong {
    display: block;
    margin-bottom: 10px;
    color: var(--text-primary);
    font-size: var(--settings-heading-size, 13px);
    font-weight: 560;
  }

  .ref-grid {
    display: grid;
    gap: 6px;
  }

  .ref-row {
    display: flex;
    align-items: center;
    gap: 5px;
    flex-wrap: wrap;
  }

  .ref-row kbd {
    display: inline-block;
    padding: 2px 7px;
    border: 1px solid var(--border-color);
    border-radius: 4px;
    color: var(--text-secondary);
    background: var(--hover-bg);
    font:
      10px "Cascadia Code",
      Consolas,
      monospace;
    line-height: 1.5;
  }

  .ref-row span {
    color: var(--text-muted);
    font-size: var(--settings-note-size, var(--font-size-tiny, 10px));
    margin-left: 6px;
  }

  .open-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 5px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-muted);
    background: var(--card-bg);
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .open-btn:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .setting-card-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .settings-subnav {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .settings-subnav button {
    min-height: 28px;
    padding: 5px 12px;
    border: 1px solid transparent;
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-muted);
    background: transparent;
    font: inherit;
    font-size: var(--settings-heading-size, 13px);
    font-weight: 560;
    cursor: pointer;
    transition:
      color 100ms ease,
      background 100ms ease,
      border-color 100ms ease;
  }

  .settings-subnav button:hover {
    border-color: var(--border-color);
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .settings-subnav button.active {
    border-color: var(--selection-color);
    color: var(--text-primary);
    background: color-mix(in srgb, var(--selection-color) 15%, transparent);
  }
</style>
