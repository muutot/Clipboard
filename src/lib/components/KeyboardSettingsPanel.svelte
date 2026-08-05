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
  let feedbackTimer: ReturnType<typeof setTimeout> | undefined;
  let recordingTimer: ReturnType<typeof setTimeout> | undefined;
  let configRequestId = 0;
  let componentDestroyed = false;

  interface SystemAction {
    id: string;
    labelKey?: string;
    descKey?: string;
    description: string;
    icon: IconName;
    defaults: string[];
    cat: typeof category;
    system?: boolean;
  }

  const SYSTEM_ACTIONS: SystemAction[] = [
    {
      id: "copyItem",
      labelKey: "keyboard.copyItem",
      descKey: "keyboard.copyItemDesc",
      description: "",
      icon: "copy",
      defaults: ["Ctrl+C", "Enter"],
      cat: "item",
    },
    {
      id: "deleteItem",
      labelKey: "keyboard.deleteItem",
      descKey: "keyboard.deleteItemDesc",
      description: "",
      icon: "trash",
      defaults: ["Ctrl+D"],
      cat: "item",
    },
    {
      id: "favoriteItem",
      labelKey: "keyboard.favoriteItem",
      descKey: "keyboard.favoriteItemDesc",
      description: "",
      icon: "star",
      defaults: ["Ctrl+F"],
      cat: "item",
    },
    {
      id: "addTag",
      labelKey: "keyboard.addTag",
      descKey: "keyboard.addTagDesc",
      description: "",
      icon: "tag",
      defaults: ["Ctrl+T"],
      cat: "item",
    },
    {
      id: "moveSelectionUp",
      labelKey: "keyboard.moveSelectionUp",
      descKey: "keyboard.moveSelectionDesc",
      description: "",
      icon: "arrow-up",
      defaults: ["Arrowup"],
      cat: "item",
    },
    {
      id: "moveSelectionDown",
      labelKey: "keyboard.moveSelectionDown",
      descKey: "keyboard.moveSelectionDesc",
      description: "",
      icon: "arrow-down",
      defaults: ["Arrowdown"],
      cat: "item",
    },
    {
      id: "switchFilterNext",
      labelKey: "keyboard.switchFilterNext",
      descKey: "keyboard.switchFilterDesc",
      description: "",
      icon: "arrow-right",
      defaults: ["Arrowright", "Tab"],
      cat: "item",
    },
    {
      id: "switchFilterPrev",
      labelKey: "keyboard.switchFilterPrev",
      descKey: "keyboard.switchFilterDesc",
      description: "",
      icon: "arrow-left",
      defaults: ["Arrowleft", "Shift+Tab"],
      cat: "item",
    },
    {
      id: "clearSelection",
      labelKey: "keyboard.clearSelection",
      descKey: "keyboard.clearSelectionDesc",
      description: "",
      icon: "x",
      defaults: ["Backspace"],
      cat: "item",
    },
    {
      id: "openDetail",
      labelKey: "keyboard.openDetail",
      descKey: "keyboard.viewDetailDesc",
      description: "",
      icon: "eye",
      defaults: ["Space", "Ctrl+E"],
      cat: "item",
    },
    {
      id: "downloadItem",
      labelKey: "keyboard.downloadItem",
      descKey: "keyboard.saveItemDesc",
      description: "",
      icon: "download",
      defaults: ["Ctrl+S"],
      cat: "item",
    },
    {
      id: "selectAll",
      labelKey: "keyboard.selectAll",
      descKey: "keyboard.selectAllDesc",
      description: "",
      icon: "check",
      defaults: ["Ctrl+A"],
      cat: "item",
    },
    {
      id: "quickPaste",
      labelKey: "keyboard.quickPaste",
      descKey: "keyboard.pasteToWindowDesc",
      description: "",
      icon: "clipboard",
      defaults: [],
      cat: "item",
    },
    {
      id: "quickCopy1",
      descKey: "keyboard.quickCopyDesc",
      description: "",
      icon: "clipboard",
      defaults: ["Ctrl+1"],
      cat: "quick",
    },
    {
      id: "quickCopy2",
      descKey: "keyboard.quickCopyDesc",
      description: "",
      icon: "clipboard",
      defaults: ["Ctrl+2"],
      cat: "quick",
    },
    {
      id: "quickCopy3",
      descKey: "keyboard.quickCopyDesc",
      description: "",
      icon: "clipboard",
      defaults: ["Ctrl+3"],
      cat: "quick",
    },
    {
      id: "quickCopy4",
      descKey: "keyboard.quickCopyDesc",
      description: "",
      icon: "clipboard",
      defaults: ["Ctrl+4"],
      cat: "quick",
    },
    {
      id: "quickCopy5",
      descKey: "keyboard.quickCopyDesc",
      description: "",
      icon: "clipboard",
      defaults: ["Ctrl+5"],
      cat: "quick",
    },
    {
      id: "quickCopy6",
      descKey: "keyboard.quickCopyDesc",
      description: "",
      icon: "clipboard",
      defaults: ["Ctrl+6"],
      cat: "quick",
    },
    {
      id: "quickCopy7",
      descKey: "keyboard.quickCopyDesc",
      description: "",
      icon: "clipboard",
      defaults: ["Ctrl+7"],
      cat: "quick",
    },
    {
      id: "quickCopy8",
      descKey: "keyboard.quickCopyDesc",
      description: "",
      icon: "clipboard",
      defaults: ["Ctrl+8"],
      cat: "quick",
    },
    {
      id: "quickCopy9",
      descKey: "keyboard.quickCopyDesc",
      description: "",
      icon: "clipboard",
      defaults: ["Ctrl+9"],
      cat: "quick",
    },
    {
      id: "toggleWindow",
      labelKey: "keyboard.toggleWindow",
      descKey: "keyboard.toggleWindowDesc",
      description: "",
      icon: "eye",
      defaults: ["Alt+V"],
      cat: "system",
      system: true,
    },
    {
      id: "hideWindow",
      labelKey: "keyboard.hideWindow",
      descKey: "keyboard.hideWindowDesc",
      description: "",
      icon: "eye",
      defaults: ["Escape"],
      cat: "system",
    },
    {
      id: "focusSearch",
      labelKey: "keyboard.focusSearch",
      descKey: "keyboard.focusSearchDesc",
      description: "",
      icon: "search",
      defaults: ["/", "Ctrl+K"],
      cat: "system",
    },
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
    if (m) return _t("keyboard.quickCopyDesc", { n: Number(m[1]) });
    return action.id;
  }

  function actionDesc(action: SystemAction): string {
    if (action.descKey) {
      const m = action.id.match(/^quickCopy(\d+)$/);
      if (m) return _t(action.descKey as keyof typeof $messages, { n: Number(m[1]) });
      return _t(action.descKey as keyof typeof $messages);
    }
    return action.description;
  }

  function bindingsFor(action: string): string[] {
    if (!config) return [];
    return config.shortcuts[action] ?? [];
  }

  const ARROW_GLYPHS: Record<string, string> = {
    Arrowup: "↑",
    Arrowdown: "↓",
    Arrowright: "→",
    Arrowleft: "←",
  };

  function arrowGlyph(shortcut: string): string | null {
    return ARROW_GLYPHS[shortcut] ?? null;
  }

  function shortcutLabel(shortcut: string): string {
    return arrowGlyph(shortcut) ?? shortcut;
  }

  onMount(() => {
    void loadConfig();
  });

  $effect(() => {
    resetToken;
    if (resetToken > 0) void loadConfig();
  });

  async function loadConfig() {
    const requestId = ++configRequestId;
    loading = true;
    if (feedbackTimer !== undefined) {
      clearTimeout(feedbackTimer);
      feedbackTimer = undefined;
    }
    feedback = "";

    try {
      const loadedConfig = await getKeyboardConfig();
      if (componentDestroyed || requestId !== configRequestId) return;
      config = loadedConfig;
      if (!loadedConfig) feedback = _t("keyboard.browserUnavailable");
    } catch (error) {
      if (!componentDestroyed && requestId === configRequestId) {
        feedback = error instanceof Error ? error.message : String(error);
      }
    } finally {
      if (!componentDestroyed && requestId === configRequestId) loading = false;
    }
  }

  function showFeedback(msg: string, success: boolean) {
    feedback = msg;
    feedbackSuccess = success;
    if (feedbackTimer !== undefined) clearTimeout(feedbackTimer);
    feedbackTimer = setTimeout(() => {
      feedbackTimer = undefined;
      feedback = "";
    }, 2000);
  }

  async function addBinding(action: string, shortcut: string) {
    if (!config) return;
    const current = config.shortcuts[action] ?? [];
    try {
      const normalized = await configureKeyboardShortcuts(action, [...current, shortcut]);
      if (componentDestroyed || !config) return;
      config = { ...config, shortcuts: { ...config.shortcuts, [action]: normalized } };
    } catch (error) {
      if (!componentDestroyed) {
        showFeedback(error instanceof Error ? error.message : String(error), false);
      }
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
      if (componentDestroyed || !config) return;
      config = { ...config, shortcuts: { ...config.shortcuts, [action]: normalized } };
    } catch (error) {
      if (!componentDestroyed) {
        showFeedback(error instanceof Error ? error.message : String(error), false);
      }
    }
  }

  function startRecording(action: string) {
    stopRecording();
    recordingAction = action;
    window.addEventListener("keydown", onRecordingKey);
    recordingTimer = setTimeout(() => {
      recordingTimer = undefined;
      stopRecording();
    }, 3000);
  }

  function stopRecording() {
    if (recordingTimer !== undefined) {
      clearTimeout(recordingTimer);
      recordingTimer = undefined;
    }
    recordingAction = "";
    window.removeEventListener("keydown", onRecordingKey);
  }

  function onRecordingKey(event: KeyboardEvent) {
    event.preventDefault();
    event.stopPropagation();

    if (event.key === "Escape") {
      stopRecording();
      return;
    }

    const modKeys = ["Control", "Alt", "Shift", "Meta"];
    if (modKeys.includes(event.key)) return;

    const pressed: string[] = [];
    if (event.ctrlKey) pressed.push("Ctrl");
    if (event.altKey) pressed.push("Alt");
    if (event.shiftKey) pressed.push("Shift");
    if (event.metaKey) pressed.push("Meta");

    const ignored = ["AltGraph", "NumLock", "ScrollLock", "PrintScreen"];
    if (!ignored.includes(event.key)) {
      pressed.push(
        event.key === " " ? "Space" : event.key.length === 1 ? event.key.toUpperCase() : event.key,
      );
    }

    if (pressed.length === 0) return;
    const action = recordingAction;
    stopRecording();
    if (action) void addBinding(action, pressed.join("+"));
  }

  onDestroy(() => {
    componentDestroyed = true;
    configRequestId += 1;
    if (feedbackTimer !== undefined) clearTimeout(feedbackTimer);
    if (recordingTimer !== undefined) clearTimeout(recordingTimer);
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
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
    >
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
                <span class="system-badge">{_t("keyboard.systemBadge")}</span>
              {/if}
            </strong>
            <p>{actionDesc(action)}</p>
          </div>
        </div>
        <div class="shortcut-bindings">
          {#if bindingsFor(action.id).length > 0}
            {#each bindingsFor(action.id) as shortcut}
              <div class="binding-chip">
                <kbd class:arrow={arrowGlyph(shortcut) !== null}>{shortcutLabel(shortcut)}</kbd>
                <button
                  type="button"
                  class="binding-chip-close"
                  onclick={() => removeBinding(action.id, shortcut)}>&minus;</button
                >
              </div>
            {/each}
          {:else if config && !(action.id in config.shortcuts) && action.defaults.length > 0}
            {#each action.defaults as shortcut}
              <div class="binding-chip default">
                <kbd class:arrow={arrowGlyph(shortcut) !== null}>{shortcutLabel(shortcut)}</kbd>
                <button
                  type="button"
                  class="binding-chip-close"
                  onclick={() => removeBinding(action.id, shortcut)}>&minus;</button
                >
              </div>
            {/each}
          {:else}
            <span class="binding-disabled">{_t("keyboard.bindingDisabled")}</span>
          {/if}

          {#if recordingAction === action.id}
            <div class="binding-chip recording">
              <kbd>{_t("keyboard.pressKey")}</kbd>
              <button type="button" class="binding-chip-close" onclick={stopRecording}
                >&times;</button
              >
            </div>
          {:else}
            <button type="button" class="binding-add" onclick={() => startRecording(action.id)}
              >+</button
            >
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
    font:
      11px "Cascadia Code",
      Consolas,
      monospace;
    color: var(--text-primary);
  }

  .binding-chip kbd.arrow {
    font-size: clamp(
      13px,
      calc(var(--settings-control-size, var(--font-size-secondary, 11px)) + 4px),
      15px
    );
    line-height: 1;
  }

  .binding-chip.recording {
    border-color: var(--selection-color);
    animation: pulse-recording 1s ease-in-out infinite;
  }

  @keyframes pulse-recording {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.5;
    }
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
    transition:
      color 100ms ease,
      border-color 100ms ease;
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
