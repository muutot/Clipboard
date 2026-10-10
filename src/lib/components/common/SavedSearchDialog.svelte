<script lang="ts">
  import { onMount } from "svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { generalSettings } from "$lib/services/settings";
  import {
    MAX_SAVED_SEARCHES,
    saveNamedSearch,
    type SavedSearch,
    type SavedSearchState,
  } from "$lib/utils/settings/saved-searches";
  let {
    current,
    onapply,
    onclose,
  }: {
    current: SavedSearchState;
    onapply: (search: SavedSearch) => void;
    onclose: () => void;
  } = $props();
  const _t = (path: string) => resolvePath($messages, path);
  let dialog: HTMLDialogElement;
  let name = $state("");
  let busy = $state(false);
  let error = $state("");
  let renaming = $state<string | null>(null);
  let renameDraft = $state("");
  onMount(() => {
    dialog.showModal();
    return () => dialog.close();
  });
  async function persist(next: SavedSearch[]) {
    busy = true;
    error = "";
    generalSettings.updateSetting("savedSearches", next);
    try {
      await generalSettings.flush();
    } catch {
      error = _t("savedSearches.failed");
    } finally {
      busy = false;
    }
  }
  async function save() {
    await persist(saveNamedSearch($generalSettings.savedSearches, name, current));
    if (!error) name = "";
  }
  async function rename(search: SavedSearch) {
    const draft = renameDraft.trim();
    if (
      !draft ||
      $generalSettings.savedSearches.some((other) => other.id !== search.id && other.name === draft)
    ) {
      error = _t("savedSearches.conflict");
      return;
    }
    await persist(
      $generalSettings.savedSearches.map((other) =>
        other.id === search.id ? { ...other, name: draft } : other,
      ),
    );
    if (!error) renaming = null;
  }
</script>

<dialog
  bind:this={dialog}
  aria-label={_t("savedSearches.title")}
  {onclose}
  oncancel={(event) => {
    event.preventDefault();
    onclose();
  }}
  onkeydown={(event) => event.stopPropagation()}
  onclick={(event) => {
    if (event.target === dialog) onclose();
  }}
>
  <header>
    <strong>{_t("savedSearches.title")}</strong><button
      onclick={onclose}
      aria-label={_t("savedSearches.close")}>×</button
    >
  </header>
  <form
    onsubmit={(event) => {
      event.preventDefault();
      void save();
    }}
  >
    <input
      bind:value={name}
      maxlength={80}
      aria-label={_t("savedSearches.name")}
      placeholder={_t("savedSearches.name")}
    />
    <button
      disabled={busy ||
        !name.trim() ||
        ($generalSettings.savedSearches.length >= MAX_SAVED_SEARCHES &&
          !$generalSettings.savedSearches.some((s) => s.name === name.trim()))}
      >{_t("savedSearches.save")}</button
    >
  </form>
  <p class="hint">{_t("savedSearches.hint")}</p>
  <div class="saved-list">
    {#each $generalSettings.savedSearches as search (search.id)}
      <div class="saved-row">
        {#if renaming === search.id}
          <form
            onsubmit={(event) => {
              event.preventDefault();
              void rename(search);
            }}
          >
            <input bind:value={renameDraft} maxlength={80} aria-label={_t("savedSearches.name")} />
            <button disabled={busy}>{_t("savedSearches.rename")}</button>
            <button type="button" onclick={() => (renaming = null)}
              >{_t("savedSearches.cancel")}</button
            >
          </form>
        {:else}
          <button
            class="apply"
            disabled={busy}
            title={search.query || search.name}
            onclick={() => {
              onapply(search);
              onclose();
            }}>{search.name}</button
          >
          <button
            disabled={busy}
            onclick={() => {
              renaming = search.id;
              renameDraft = search.name;
            }}>{_t("savedSearches.rename")}</button
          >
          <button
            disabled={busy}
            onclick={() =>
              persist($generalSettings.savedSearches.filter((s) => s.id !== search.id))}
            >{_t("savedSearches.remove")}</button
          >
        {/if}
      </div>
    {:else}
      <p class="hint">{_t("savedSearches.empty")}</p>
    {/each}
  </div>
  {#if error}<p role="alert">{error}</p>
    <button disabled={busy} onclick={() => persist($generalSettings.savedSearches)}
      >{_t("savedSearches.retry")}</button
    >{/if}
</dialog>

<style>
  dialog {
    margin: auto;
    padding: 12px;
    width: min(420px, calc(100vw - 24px));
    max-height: calc(100vh - 24px);
    border: 1px solid var(--border-color);
    border-radius: 10px;
    color: var(--text-primary);
    background: var(--surface-bg);
  }
  dialog::backdrop {
    background: color-mix(in srgb, var(--bg-app) 60%, transparent);
  }
  header,
  form,
  .saved-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  header {
    justify-content: space-between;
    margin-bottom: 10px;
  }
  form {
    flex: 1;
    min-width: 0;
  }
  input {
    flex: 1;
    min-width: 0;
    padding: 6px;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-primary);
    background: var(--input-bg);
  }
  button {
    padding: 6px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: var(--font-size-secondary);
  }
  button:hover {
    background: var(--hover-bg);
    color: var(--text-primary);
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .saved-list {
    max-height: 45vh;
    overflow: auto;
  }
  .saved-row {
    padding: 4px 0;
  }
  .apply {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }
  .hint {
    color: var(--text-muted);
    font-size: var(--font-size-secondary);
    margin: 8px 0;
  }
  [role="alert"] {
    color: var(--danger-color);
  }
</style>
