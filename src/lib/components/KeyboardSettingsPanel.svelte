<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import {
    configureKeyboardShortcuts,
    getKeyboardConfig,
    type KeyboardConfig,
  } from "$lib/services/keyboard";

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

  const actionLabels: Record<string, string> = {
    toggleWindow: "唤起或隐藏主窗口",
    quickPaste: "快速粘贴当前条目",
  };

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
        feedback = "浏览器预览无法读取桌面端快捷键配置";
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
      feedback = `已保存 ${normalized.length} 组快捷键，运行时立即生效`;
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
    <span class="eyebrow">设置 / 快捷键</span>
    <h2>键盘与唤起</h2>
    <p>每个操作可绑定多个组合；双击修饰键写作 Shift+Shift、Ctrl+Ctrl。</p>
  </div>
  <button class="close-button" type="button" aria-label="关闭设置" onclick={onclose}>×</button>
</header>

{#if loading}
  <div class="settings-state">正在读取快捷键配置…</div>
{:else if config}
  <div class="settings-scroll">
    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="keyboard" size={17} /></span>
        <div>
          <strong>独立快捷键配置</strong>
          <p>常规设置与快捷键分文件保存，切换数据目录时都不会迁移。</p>
        </div>
      </div>
      <code class="path-value" title={configPath}>{configPath}</code>
    </section>

    {#each Object.entries(config.shortcuts) as [action]}
      <section class="setting-card">
        <div class="setting-heading split-heading">
          <div>
            <strong>{actionLabel(action)}</strong>
            <p><code>{action}</code> · 多组绑定使用逗号分隔</p>
          </div>
          <span class="binding-count">{config.shortcuts[action].length} 组</span>
        </div>
        <label for={`shortcut-${action}`}>快捷键组合</label>
        <input
          id={`shortcut-${action}`}
          value={drafts[action] ?? ""}
          oninput={(event) => (drafts[action] = event.currentTarget.value)}
          autocomplete="off"
          spellcheck="false"
          placeholder="例如 Alt+V, Shift+Shift"
        />
        <div class="setting-actions">
          <button
            class="primary"
            type="button"
            disabled={savingAction !== ""}
            onclick={() => saveAction(action)}
          >
            {savingAction === action ? "保存中…" : "保存绑定"}
          </button>
        </div>
      </section>
    {/each}

    <section class="shortcut-help">
      <strong>格式提示</strong>
      <span>普通组合：Ctrl+Shift+V</span>
      <span>双击修饰键：Shift+Shift</span>
      <span>同一组合不能分配给两个操作</span>
    </section>
  </div>
{:else}
  <div class="settings-state">{feedback || "桌面端快捷键服务不可用"}</div>
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

  .eyebrow { color: #777; font-size: 9.5px; letter-spacing: 0.08em; text-transform: uppercase; }
  h2 { margin: 5px 0 4px; color: #efefef; font-size: 18px; font-weight: 590; }
  header p, .setting-heading p { margin: 0; color: #777; line-height: 1.5; }
  header p { max-width: 430px; font-size: 10.5px; }

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

  .setting-card { padding: 13px; border: 1px solid #303030; border-radius: 9px; background: #1e1e1e; }
  .setting-heading { display: flex; align-items: center; gap: 10px; }
  .split-heading { justify-content: space-between; }
  .setting-icon { display: inline-flex; align-items: center; justify-content: center; width: 29px; height: 29px; flex: 0 0 auto; border: 1px solid #363636; border-radius: 7px; color: #d2d2d2; background: #242424; }
  .setting-heading strong { display: block; color: #dedede; font-size: 11.5px; font-weight: 560; }
  .setting-heading p { margin-top: 2px; font-size: 9.8px; }
  .setting-heading code { color: #888; }

  .path-value { display: block; overflow: hidden; margin-top: 11px; padding: 8px 9px; border: 1px solid #2f2f2f; border-radius: 6px; color: #a7a7a7; background: #181818; font: 9.5px "Cascadia Code", Consolas, monospace; white-space: nowrap; text-overflow: ellipsis; }
  .binding-count { flex: 0 0 auto; padding: 3px 7px; border: 1px solid #393939; border-radius: 999px; color: #999; font-size: 9px; }
  label { display: block; margin: 12px 0 6px; color: #8a8a8a; font-size: 9.5px; }
  input { width: 100%; box-sizing: border-box; padding: 9px 10px; border: 1px solid #343434; border-radius: 7px; outline: none; color: #d7d7d7; background: #171717; font: 10.5px "Cascadia Code", Consolas, monospace; }
  input:focus { border-color: #555; }

  .setting-actions { display: flex; justify-content: flex-end; margin-top: 9px; }
  .setting-actions button { padding: 7px 10px; border: 1px solid #e3e3e3; border-radius: 6px; color: #1c1c1c; background: #e3e3e3; font: inherit; font-size: 10px; }
  button { cursor: pointer; }
  button:disabled { cursor: wait; opacity: 0.55; }

  .shortcut-help { display: flex; flex-wrap: wrap; gap: 7px 12px; padding: 4px 3px; color: #686868; font-size: 9.5px; }
  .shortcut-help strong { color: #898989; }
  .settings-state { display: grid; flex: 1; place-items: center; color: #777; font-size: 11px; }
  .settings-feedback { position: absolute; right: 18px; bottom: 13px; left: 18px; padding: 8px 10px; border: 1px solid #553434; border-radius: 7px; color: #d59c9c; background: rgba(48, 27, 27, 0.96); font-size: 10px; }
  .settings-feedback.success { border-color: #35513f; color: #9dc6aa; background: rgba(27, 45, 33, 0.96); }
</style>
