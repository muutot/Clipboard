<script lang="ts">
  import { onMount } from "svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import {
    configureIgnoredApplications,
    getApplicationFilterSettings,
    type ApplicationFilterSettings,
  } from "$lib/services/capture";

  interface Props {
    configPath?: string;
    onclose: () => void;
  }

  let { configPath = "conf/conf.json", onclose }: Props = $props();
  let settings = $state<ApplicationFilterSettings | null>(null);
  let availableSearch = $state("");
  let ignoredSearch = $state("");
  let selectedAvailable = $state<string[]>([]);
  let manualApplication = $state("");
  let loading = $state(true);
  let saving = $state(false);
  let feedback = $state("");
  let feedbackSuccess = $state(false);

  const ignoredKeys = $derived(
    new Set((settings?.ignoredApplications ?? []).map(normalizeApplication)),
  );
  const availableApplications = $derived(
    (settings?.discoveredApplications ?? []).filter(
      (application) => !ignoredKeys.has(normalizeApplication(application)),
    ),
  );
  const visibleAvailable = $derived(
    filterApplications(availableApplications, availableSearch),
  );
  const visibleIgnored = $derived(
    filterApplications(settings?.ignoredApplications ?? [], ignoredSearch),
  );

  onMount(() => {
    void loadSettings();
  });

  async function loadSettings() {
    loading = true;
    feedback = "";
    feedbackSuccess = false;

    try {
      settings = await getApplicationFilterSettings();
      if (!settings) feedback = "浏览器预览无法读取桌面端应用列表";
    } catch (error) {
      console.error("Unable to load application filters", error);
      feedback = error instanceof Error ? error.message : String(error);
    } finally {
      loading = false;
    }
  }

  function toggleAvailable(application: string) {
    selectedAvailable = selectedAvailable.includes(application)
      ? selectedAvailable.filter((candidate) => candidate !== application)
      : [...selectedAvailable, application];
  }

  async function ignoreSelected() {
    if (!settings || selectedAvailable.length === 0) return;
    await saveIgnored([...settings.ignoredApplications, ...selectedAvailable]);
    selectedAvailable = [];
  }

  async function removeIgnored(application: string) {
    if (!settings) return;
    await saveIgnored(
      settings.ignoredApplications.filter((candidate) => candidate !== application),
    );
  }

  async function addManualApplication() {
    const application = manualApplication.trim();
    if (!settings || !application) return;
    await saveIgnored([...settings.ignoredApplications, application]);
    manualApplication = "";
  }

  async function saveIgnored(applications: string[]) {
    if (!settings) return;
    saving = true;
    feedback = "";
    feedbackSuccess = false;

    try {
      const ignoredApplications = await configureIgnoredApplications(applications);
      settings = { ...settings, ignoredApplications };
      feedback = `已保存 ${ignoredApplications.length} 个忽略应用`;
      feedbackSuccess = true;
    } catch (error) {
      console.error("Unable to save ignored applications", error);
      feedback = error instanceof Error ? error.message : String(error);
    } finally {
      saving = false;
    }
  }

  function filterApplications(applications: string[], query: string): string[] {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    if (!normalizedQuery) return applications;
    return applications.filter((application) =>
      application.toLocaleLowerCase().includes(normalizedQuery),
    );
  }

  function normalizeApplication(application: string): string {
    return application.trim().toLocaleLowerCase();
  }
</script>

<header>
  <div>
    <span class="eyebrow">设置 / 采集 / 应用过滤</span>
    <h2>忽略的应用</h2>
    <p>来自忽略应用的剪贴板内容不会进入历史、文件存储、OCR 队列或搜索索引。</p>
  </div>
  <button class="close-button" type="button" aria-label="关闭设置" onclick={onclose}>×</button>
</header>

{#if loading}
  <div class="settings-state">正在读取来源应用…</div>
{:else if settings}
  <div class="settings-scroll">
    <section class="filter-board">
      <div class="application-column">
        <div class="column-heading">
          <strong>可忽略的应用 <span>{availableApplications.length} 项</span></strong>
          <button type="button" title="刷新来源应用" aria-label="刷新来源应用" onclick={loadSettings}>↻</button>
        </div>
        <label class="search-field">
          <AppIcon name="search" size={15} />
          <input bind:value={availableSearch} placeholder="搜索应用" />
        </label>
        <div class="application-list">
          {#each visibleAvailable as application}
            <label class="application-row">
              <input
                type="checkbox"
                checked={selectedAvailable.includes(application)}
                onchange={() => toggleAvailable(application)}
              />
              <span class="app-avatar">{application.slice(0, 1).toLocaleUpperCase()}</span>
              <strong>{application}</strong>
            </label>
          {:else}
            <p class="empty-list">尚未发现可配置的来源应用</p>
          {/each}
        </div>
      </div>

      <div class="transfer-column">
        <button
          type="button"
          aria-label="忽略选中的应用"
          title="忽略选中的应用"
          disabled={saving || selectedAvailable.length === 0}
          onclick={ignoreSelected}
        >›</button>
      </div>

      <div class="application-column">
        <div class="column-heading">
          <strong>已忽略的应用 <span>{settings.ignoredApplications.length} 项</span></strong>
          <span class="plus-mark">＋</span>
        </div>
        <label class="search-field">
          <AppIcon name="search" size={15} />
          <input bind:value={ignoredSearch} placeholder="搜索应用" />
        </label>
        <div class="manual-add">
          <input
            bind:value={manualApplication}
            placeholder="手动添加应用名称"
            onkeydown={(event) => event.key === "Enter" && addManualApplication()}
          />
          <button type="button" disabled={saving || !manualApplication.trim()} onclick={addManualApplication}>添加</button>
        </div>
        <div class="application-list">
          {#each visibleIgnored as application}
            <div class="application-row ignored-row">
              <span class="app-avatar locked">{application.slice(0, 1).toLocaleUpperCase()}</span>
              <strong>{application}</strong>
              <button
                type="button"
                aria-label={`不再忽略 ${application}`}
                title="移出忽略列表"
                disabled={saving}
                onclick={() => removeIgnored(application)}
              >×</button>
            </div>
          {:else}
            <p class="empty-list">当前没有匹配的忽略应用</p>
          {/each}
        </div>
      </div>
    </section>

    <div class="settings-note">
      <span>配置文件</span>
      <code title={configPath}>{configPath}</code>
      <span>· 应用来源识别接入后会自动补充左侧列表</span>
    </div>
  </div>
{:else}
  <div class="settings-state">{feedback || "桌面端应用过滤服务不可用"}</div>
{/if}

{#if feedback && settings}
  <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
{/if}

<style>
  header { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; padding: 20px 24px 15px; border-bottom: 1px solid #292929; }
  .eyebrow { color: #777; font-size: 9.5px; letter-spacing: 0.08em; text-transform: uppercase; }
  h2 { margin: 5px 0 4px; color: #efefef; font-size: 18px; font-weight: 590; }
  header p { max-width: 570px; margin: 0; color: #777; font-size: 10.5px; line-height: 1.5; }
  .close-button { width: 28px; height: 28px; border: 1px solid #353535; border-radius: 7px; color: #999; background: #222; font-size: 18px; line-height: 1; }
  .settings-scroll { display: grid; gap: 10px; min-height: 0; padding: 16px 20px 48px; overflow: auto; }
  .filter-board { display: grid; grid-template-columns: minmax(0, 1fr) 30px minmax(0, 1fr); min-height: 365px; border: 1px solid #303030; border-radius: 10px; background: #1b1b1b; }
  .application-column { display: grid; grid-template-rows: auto auto auto minmax(0, 1fr); min-width: 0; padding: 13px; }
  .application-column:first-child { grid-template-rows: auto auto minmax(0, 1fr); }
  .application-column + .transfer-column, .transfer-column + .application-column { border-left: 1px solid #303030; }
  .column-heading { display: flex; align-items: center; justify-content: space-between; min-height: 25px; color: #ddd; font-size: 11px; }
  .column-heading strong span { margin-left: 4px; padding: 2px 6px; border: 1px solid #3a3a3a; border-radius: 999px; color: #898989; font-size: 8.5px; font-weight: 500; }
  .column-heading button, .plus-mark { border: 0; color: #888; background: transparent; font-size: 16px; }
  .search-field { display: flex; align-items: center; gap: 7px; margin: 9px 0; padding: 7px 9px; border: 1px solid #373737; border-radius: 7px; color: #676767; background: #181818; }
  .search-field input, .manual-add input { min-width: 0; flex: 1; border: 0; outline: 0; color: #d7d7d7; background: transparent; font: 10.5px inherit; }
  .manual-add { display: flex; gap: 6px; margin-bottom: 8px; padding: 6px 7px 6px 9px; border: 1px dashed #363636; border-radius: 7px; background: #191919; }
  .manual-add button { border: 0; border-radius: 5px; padding: 4px 7px; color: #c8c8c8; background: #303030; font: inherit; font-size: 9px; }
  .application-list { min-height: 0; overflow: auto; }
  .application-row { display: flex; align-items: center; gap: 8px; min-height: 36px; padding: 3px 5px; border-radius: 7px; color: #d8d8d8; font-size: 10.5px; }
  .application-row:hover { background: #242424; }
  .application-row > input { width: 14px; height: 14px; accent-color: #4d8dff; }
  .application-row strong { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .application-row > button { margin-left: auto; border: 0; color: #666; background: transparent; font-size: 16px; }
  .app-avatar { display: inline-grid; width: 25px; height: 25px; flex: 0 0 auto; place-items: center; border: 1px solid #424242; border-radius: 7px; color: #ddd; background: linear-gradient(145deg, #3d4656, #252a32); font-size: 10px; font-weight: 700; }
  .app-avatar.locked { color: #c9d7ff; background: linear-gradient(145deg, #3d4770, #242a44); }
  .transfer-column { display: grid; place-items: center; }
  .transfer-column button { width: 24px; height: 28px; border: 1px solid #3d3d3d; border-radius: 6px; color: #aaa; background: #292929; font-size: 18px; }
  button { cursor: pointer; }
  button:disabled { cursor: default; opacity: 0.35; }
  .empty-list { margin: 16px 6px; color: #626262; font-size: 9.5px; text-align: center; }
  .settings-note { display: flex; align-items: center; gap: 7px; min-width: 0; padding: 2px 4px; color: #666; font-size: 9.5px; }
  .settings-note code { overflow: hidden; color: #888; white-space: nowrap; text-overflow: ellipsis; }
  .settings-state { display: grid; flex: 1; place-items: center; color: #777; font-size: 11px; }
  .settings-feedback { position: absolute; right: 18px; bottom: 13px; left: 18px; padding: 8px 10px; border: 1px solid #553434; border-radius: 7px; color: #d59c9c; background: rgba(48, 27, 27, 0.96); font-size: 10px; }
  .settings-feedback.success { border-color: #35513f; color: #9dc6aa; background: rgba(27, 45, 33, 0.96); }
  @media (max-width: 700px) { .filter-board { grid-template-columns: 1fr; } .transfer-column { min-height: 38px; border-top: 1px solid #303030; border-left: 0 !important; } .transfer-column button { transform: rotate(90deg); } .transfer-column + .application-column { border-top: 1px solid #303030; border-left: 0; } }
</style>
