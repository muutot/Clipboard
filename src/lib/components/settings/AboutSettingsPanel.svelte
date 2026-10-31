<script lang="ts">
  import AppIcon from "$lib/components/card/AppIcon.svelte";
  import CustomSelect from "$lib/components/common/CustomSelect.svelte";
  import UpdateDialog from "$lib/components/settings/UpdateDialog.svelte";
  import EntryHeading from "$lib/components/settings/settings-entries/EntryHeading.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { generalSettings } from "$lib/services/settings";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { checkForUpdate, getRelease, type UpdateInfo } from "$lib/services/update";
  import { invoke } from "@tauri-apps/api/core";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    appVersion: string;
    appExecutablePath: string;
    onclose: () => void;
  }

  let { appVersion, appExecutablePath, onclose }: Props = $props();

  let checkingUpdate = $state(false);
  let updateResult = $state<UpdateInfo | null>(null);
  let updateError = $state("");
  let showUpdateDialog = $state(false);
  let dialogMode: "current" | "available" = $state("available");
  let loadingRelease = $state(false);

  async function handleViewRelease(): Promise<void> {
    if (!isTauriRuntime() || loadingRelease || !appVersion) return;
    loadingRelease = true;
    try {
      updateResult = await getRelease(appVersion);
      dialogMode = "current";
      showUpdateDialog = true;
    } catch (error) {
      updateError = error instanceof Error ? error.message : String(error);
    } finally {
      loadingRelease = false;
    }
  }

  async function handleCheckUpdate(): Promise<void> {
    if (!isTauriRuntime() || checkingUpdate) return;
    checkingUpdate = true;
    updateResult = null;
    updateError = "";
    try {
      updateResult = await checkForUpdate();
      if (updateResult.updateAvailable) {
        dialogMode = "available";
        showUpdateDialog = true;
      }
    } catch (error) {
      updateError = error instanceof Error ? error.message : String(error);
    } finally {
      checkingUpdate = false;
    }
  }
</script>

<div class="settings-scroll">
  <section class="setting-card toggle-card" data-settings-search-id="about.info">
    <div class="setting-heading">
      <span class="brand-icon"><AppIcon name="clipboard" size={18} /></span>
      <div>
        <strong>{_t("app.name")}</strong>
        <p>{_t("about.versionLabel", { version: appVersion })}</p>
      </div>
    </div>
    <div class="about-update-controls" data-settings-search-id="about.update">
      <CustomSelect
        value={$generalSettings.updateSource}
        ariaLabel={_t("about.updateSource")}
        options={[
          { value: "gitcode", label: _t("about.updateSourceGitcode") },
          { value: "github", label: _t("about.updateSourceGithub") },
        ]}
        onchange={(v) => generalSettings.updateSetting("updateSource", v as "gitcode" | "github")}
      />
      <button
        type="button"
        class="settings-action-btn"
        disabled={!appVersion || loadingRelease}
        onclick={handleViewRelease}
      >
        {loadingRelease ? _t("about.loadingReleaseNotes") : _t("about.releaseNotes")}
      </button>
      <button
        type="button"
        class="settings-action-btn"
        disabled={checkingUpdate}
        onclick={handleCheckUpdate}
      >
        {checkingUpdate ? _t("about.checking") : _t("about.checkUpdate")}
      </button>
    </div>
  </section>

  <section class="setting-card">
    <EntryHeading
      icon="file"
      label={_t("about.executablePathTitle")}
      desc={appExecutablePath || _t("about.executablePathEmpty")}
      descClass="about-path"
    />
  </section>

  <section class="setting-card toggle-card">
    <EntryHeading icon="code" label={_t("about.repoTitle")} desc={_t("about.repoDesc")} />
    <div class="about-update-controls">
      <button
        type="button"
        class="settings-action-btn"
        onclick={() =>
          invoke("open_external_url", { url: "https://github.com/muutot/Clipboard" }).catch((err) =>
            console.warn("open_external_url failed:", err),
          )}
      >
        GitHub
      </button>
      <button
        type="button"
        class="settings-action-btn"
        onclick={() =>
          invoke("open_external_url", { url: "https://gitcode.com/m2u/Clipboard" }).catch((err) =>
            console.warn("open_external_url failed:", err),
          )}
      >
        GitCode
      </button>
    </div>
  </section>

  <section class="setting-card toggle-card" data-settings-search-id="about.devtools">
    <EntryHeading icon="code" label={_t("about.devtoolsTitle")} desc={_t("about.devtoolsDesc")} />
    <button
      type="button"
      class="toggle-switch"
      class:active={$generalSettings.devtoolsShortcutEnabled}
      role="switch"
      aria-checked={$generalSettings.devtoolsShortcutEnabled}
      aria-label={_t("about.devtoolsTitle")}
      onclick={() =>
        generalSettings.updateSetting(
          "devtoolsShortcutEnabled",
          !$generalSettings.devtoolsShortcutEnabled,
        )}><span class="toggle-knob"></span></button
    >
  </section>

  {#if updateResult}
    {#if !updateResult.updateAvailable}
      <div class="about-update-state" role="status">
        <AppIcon name="check" size={14} />
        <span>{_t("about.upToDate")}</span>
      </div>
    {/if}
  {:else if updateError}
    <div class="about-update-state about-update-state--fail" role="alert">
      <AppIcon name="x" size={14} />
      <span>{_t("about.checkFailed", { error: updateError })}</span>
    </div>
  {/if}
  {#if showUpdateDialog && updateResult}
    <UpdateDialog
      result={updateResult}
      mode={dialogMode}
      onclose={() => (showUpdateDialog = false)}
    />
  {/if}
</div>

<style>
  .brand-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    border: 1px solid var(--border-color);
    color: var(--text-secondary);
    background: transparent;
    width: 32px;
    height: 32px;
    border-radius: 9px;
  }

  .about-update-controls {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 0 0 auto;
  }

  .about-update-controls :global(.settings-select) {
    height: 28px;
  }

  /* The <p> renders inside EntryHeading, so the panel class needs :global(). */
  .setting-card :global(.about-path) {
    overflow-wrap: anywhere;
    word-break: break-all;
  }

  .about-update-state {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
    padding: 7px 10px;
    border: 1px solid color-mix(in srgb, var(--success-color) 35%, transparent);
    border-radius: var(--settings-control-radius);
    color: color-mix(in srgb, var(--success-color) 75%, white);
    background: color-mix(in srgb, var(--success-color) 12%, var(--surface-bg));
    font-size: var(--settings-description-size);
  }

  .about-update-state--fail {
    border-color: color-mix(in srgb, var(--danger-color) 35%, transparent);
    color: color-mix(in srgb, var(--danger-color) 75%, white);
    background: color-mix(in srgb, var(--danger-color) 12%, var(--surface-bg));
  }
</style>
