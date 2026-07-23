<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import {
    configureKeyboardShortcuts,
    getKeyboardConfig,
    type KeyboardConfig,
  } from "$lib/services/keyboard";
  import { messages, resolvePath } from "$lib/i18n";

  const _t = (
    path: string,
    params?: Record<string, string | number>,
  ) => resolvePath($messages, path, params);

  interface Props {
    configPath?: string;
    onclose: () => void;
  }

  let { configPath = "conf/keyboard.json", onclose }: Props = $props();
  let config = $state<KeyboardConfig | null>(null);
  let drafts = $state<Record<string, string>>({});
  let loading = $state(true);
  let savingAction = $state("");
  let feedback = $state("");
  let feedbackSuccess = $state(false);

  const actionLabels: Record<string, string> = $derived({
    toggleWindow: _t("keyboard.toggleWindow"),
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

<header>
  <div>
    <span class="eyebrow">{_t("keyboard.settings")}</span>
    <h2>{_t("keyboard.title")}</h2>
    <p>每个操作可绑定多个组合；双击修饰键写�?Shift+Shift、Ctrl+Ctrl�?/p>
  </div>
  <button
    class="close-button"
    type="button"
    aria-label={_t("actions.close")}
    onclick={onclose}>×</button
  >
</header>

{#if loading}
  <div class="settings-state">{_t("keyboard.readingConfig")}</div>
{:else if config}
  <div class="settings-scroll">
    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="keyboard" size={17} /></span>
        <div>
          <strong>{_t("keyboard.shortcutConfigTitle")}</strong>
          <p>{_t("keyboard.shortcutConfigDesc")}</p>
        </div>
      </div>
      <code class="path-value" title={configPath}>{configPath}</code>
    </section>

    {#each Object.entries(config.shortcuts) as [action]}
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

    <section class="shortcut-help">
      <strong>{_t("keyboard.formatHint")}</strong>
      <span>{_t("keyboard.chordFormat")}</span>
      <span>{_t("keyboard.doubleFormat")}</span>
      <span>{_t("keyboard.noDuplicate")}</span>
    </section>

    <section class="shortcut-reference">
      <strong>当前快捷键参考</strong>
      <div class="ref-grid">
        <div class="ref-row"><kbd>↑</kbd><kbd>↓</kbd><span>选择条目</span></div>
        <div class="ref-row"><kbd>←</kbd><kbd>→</kbd><kbd>Tab</kbd><span>切换分类</span></div>
        <div class="ref-row"><kbd>Enter</kbd><span>激活</span></div>
        <div class="ref-row"><kbd>ESC</kbd><span>隐藏窗口</span></div>
        <div class="ref-row"><kbd>Ctrl</kbd>+<kbd>C</kbd><kbd>D</kbd><kbd>F</kbd><kbd>E</kbd><span>复制 / 删除 / 收藏 / 编辑</span></div>
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
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    padding: 20px 22px 15px;
    border-bottom: 1px solid #292929;
  }

  .eyebrow {
    color: #777;
    font-size: 9.5px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  h2 {
    margin: 5px 0 4px;
    color: #efefef;
    font-size: 18px;
    font-weight: 590;
  }
  header p,
  .setting-heading p {
    margin: 0;
    color: #777;
    line-height: 1.5;
  }
  header p {
    max-width: 430px;
    font-size: 10.5px;
  }

  .close-button {
    width: 28px;
    height: 28px;
    border: 1px solid #353535;
    border-radius: 7px;
    color: #999;
    background: #222;
    font-size: 18px;
    line-height: 1;
  }

  .settings-scroll {
    display: grid;
    gap: 10px;
    min-height: 0;
    padding: 14px 18px 48px;
    overflow: auto;
  }

  .setting-card {
    padding: 13px;
    border: 1px solid #303030;
    border-radius: 9px;
    background: #1e1e1e;
  }
  .setting-heading {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .split-heading {
    justify-content: space-between;
  }
  .setting-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 29px;
    height: 29px;
    flex: 0 0 auto;
    border: 1px solid #363636;
    border-radius: 7px;
    color: #d2d2d2;
    background: #242424;
  }
  .setting-heading strong {
    display: block;
    color: #dedede;
    font-size: 11.5px;
    font-weight: 560;
  }
  .setting-heading p {
    margin-top: 2px;
    font-size: 9.8px;
  }
  .setting-heading code {
    color: #888;
  }

  .path-value {
    display: block;
    overflow: hidden;
    margin-top: 11px;
    padding: 8px 9px;
    border: 1px solid #2f2f2f;
    border-radius: 6px;
    color: #a7a7a7;
    background: #181818;
    font:
      9.5px "Cascadia Code",
      Consolas,
      monospace;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .binding-count {
    flex: 0 0 auto;
    padding: 3px 7px;
    border: 1px solid #393939;
    border-radius: 999px;
    color: #999;
    font-size: 9px;
  }
  label {
    display: block;
    margin: 12px 0 6px;
    color: #8a8a8a;
    font-size: 9.5px;
  }
  input {
    width: 100%;
    box-sizing: border-box;
    padding: 9px 10px;
    border: 1px solid #343434;
    border-radius: 7px;
    outline: none;
    color: #d7d7d7;
    background: #171717;
    font:
      10.5px "Cascadia Code",
      Consolas,
      monospace;
  }
  input:focus {
    border-color: #555;
  }

  .setting-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 9px;
  }
  .setting-actions button {
    padding: 7px 10px;
    border: 1px solid #e3e3e3;
    border-radius: 6px;
    color: #1c1c1c;
    background: #e3e3e3;
    font: inherit;
    font-size: 10px;
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    cursor: wait;
    opacity: 0.55;
  }

  .shortcut-help {
    display: flex;
    flex-wrap: wrap;
    gap: 7px 12px;
    padding: 4px 3px;
    color: #686868;
    font-size: 9.5px;
  }
  .shortcut-help strong {
    color: #898989;
  }
  .settings-state {
    display: grid;
    flex: 1;
    place-items: center;
    color: #777;
    font-size: 11px;
  }
  .settings-feedback {
    position: absolute;
    right: 18px;
    bottom: 13px;
    left: 18px;
    padding: 8px 10px;
    border: 1px solid #553434;
    border-radius: 7px;
    color: #d59c9c;
    background: rgba(48, 27, 27, 0.96);
    font-size: 10px;
  }
  .settings-feedback.success {
    border-color: #35513f;
    color: #9dc6aa;
    background: rgba(27, 45, 33, 0.96);
  }

  .auto-save-note {
    margin: 0;
    padding: 8px 0 0;
    color: #666;
    font-size: 10px;
    text-align: center;
  }

  .shortcut-reference {
    padding: 13px;
    border: 1px solid #303030;
    border-radius: 9px;
    background: #1e1e1e;
  }

  .shortcut-reference strong {
    display: block;
    margin-bottom: 10px;
    color: #dedede;
    font-size: 11.5px;
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
    border: 1px solid #4a4a4a;
    border-radius: 4px;
    color: #c3c3c3;
    background: #252525;
    font:
      10px "Cascadia Code",
      Consolas,
      monospace;
    line-height: 1.5;
  }

  .ref-row span {
    color: #888;
    font-size: 10.5px;
    margin-left: 6px;
  }
</style>
