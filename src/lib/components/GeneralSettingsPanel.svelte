<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath, locale } from "$lib/i18n";
  import type { Locale } from "$lib/i18n/types";
  import type {
    CardActionsDisplay,
    SearchSuggestionMode,
    SortRule,
    WindowConfig,
  } from "$lib/types/clipboard";
  import { generalSettings, getWindowConfig, setWindowConfig } from "$lib/services/settings";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  let _cachedWindowConfig: WindowConfig | null = null;

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
    section?: "search" | "items" | "display" | "window";
  }

  let { onclose, showHeader = true, section = "search" }: Props = $props();

  let s = $state($generalSettings);
  let feedback = $state("");
  let feedbackSuccess = $state(false);
  let windowConfig = $state<WindowConfig | null>(
    _cachedWindowConfig ?? { launchAtStartup: false, closeToTray: true, singleInstance: true },
  );
  let windowConfigLoading = $state(!_cachedWindowConfig);
  let windowConfigSaving = $state(false);
  let sortDragIdx = $state<number | null>(null);
  let sortDragOverIdx = $state<number | null>(null);
  let sortListEl = $state<HTMLDivElement | null>(null);

  const ALL_SORT_FIELDS: SortRule["field"][] = [
    "createdAt",
    "lastUsedAt",
    "title",
    "size",
    "kind",
    "favorite",
  ];

  const SORT_FIELD_LABELS: Record<SortRule["field"], string> = {
    createdAt: "general.sortFieldCreatedAt",
    lastUsedAt: "general.sortFieldLastUsedAt",
    title: "general.sortFieldTitle",
    size: "general.sortFieldSize",
    kind: "general.sortFieldKind",
    favorite: "general.sortFieldFavorite",
  };

  function pointerDragStart(idx: number, _e: PointerEvent) {
    sortDragIdx = idx;
    sortDragOverIdx = null;
    const rows = sortListEl?.querySelectorAll<HTMLElement>(".sort-rule-row");

    function onMove(ev: PointerEvent) {
      if (!rows || rows.length === 0) return;
      let target: number | null = null;
      for (let i = 0; i < rows.length; i++) {
        const rect = rows[i].getBoundingClientRect();
        if (ev.clientY >= rect.top && ev.clientY <= rect.bottom) {
          target = i;
          break;
        }
      }
      for (let i = 0; i < rows.length; i++) {
        if (target !== null && i === target && i !== sortDragIdx) {
          rows[i].classList.add("sort-drag-over");
        } else {
          rows[i].classList.remove("sort-drag-over");
        }
      }
    }

    function onUp() {
      const target = [...(rows ?? [])].findIndex((r) => r.classList.contains("sort-drag-over"));
      if (target !== -1 && target !== sortDragIdx) {
        moveSortRule(sortDragIdx!, target);
      }
      rows?.forEach((r) => r.classList.remove("sort-drag-over"));
      sortDragIdx = null;
      sortDragOverIdx = null;
      document.removeEventListener("pointermove", onMove);
      document.removeEventListener("pointerup", onUp);
    }

    document.addEventListener("pointermove", onMove);
    document.addEventListener("pointerup", onUp);
  }

  function moveSortRule(fromIdx: number, toIdx: number) {
    if (fromIdx === toIdx) return;
    const newRules = [...s.searchSortRules];
    const [removed] = newRules.splice(fromIdx, 1);
    newRules.splice(toIdx, 0, removed);
    generalSettings.updateSetting("searchSortRules", newRules);
  }

  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
    });
    return unsub;
  });

  $effect(() => {
    let cancelled = false;
    void getWindowConfig()
      .then((config) => {
        if (!cancelled) {
          _cachedWindowConfig = config;
          windowConfig = config;
        }
      })
      .catch(() => {
        if (!cancelled) showFeedback(_t("general.windowConfigLoadFailed"), false);
      })
      .finally(() => {
        if (!cancelled) windowConfigLoading = false;
      });
    return () => {
      cancelled = true;
    };
  });

  function showFeedback(message: string, success: boolean) {
    feedback = message;
    feedbackSuccess = success;
    setTimeout(() => (feedback = ""), 2000);
  }

  function changeLanguage(lang: Locale) {
    generalSettings.updateSetting("language", lang);
    locale.set(lang);
    showFeedback(_t(lang === "zh-CN" ? "已切换至中文" : "Switched to English"), true);
  }

  function handleTransparency(event: Event) {
    const val = Number((event.target as HTMLInputElement).value);
    generalSettings.updateSetting("windowTransparency", val);
    updateSliderTrack(transparencyEl);
  }

  async function changeWindowSetting(key: "launchAtStartup" | "closeToTray", value: boolean) {
    if (!windowConfig || windowConfigSaving) return;
    const previous = windowConfig;
    windowConfig = { ...previous, [key]: value };
    windowConfigSaving = true;
    try {
      await setWindowConfig({ [key]: value });
    } catch {
      windowConfig = previous;
      showFeedback(_t("general.windowConfigUpdateFailed"), false);
    } finally {
      windowConfigSaving = false;
    }
  }

  function updateSliderTrack(el: HTMLInputElement | null) {
    if (!el) return;
    const range = Number(el.max) - Number(el.min);
    const pct = range > 0 ? ((Number(el.value) - Number(el.min)) / range) * 100 : 100;
    el.style.setProperty("--slider-pct", pct + "%");
  }

  let transparencyEl = $state<HTMLInputElement | null>(null);
  let viewerOpacityEl = $state<HTMLInputElement | null>(null);
  let maxTextLinesEl = $state<HTMLInputElement | null>(null);
  let pageSizeEl = $state<HTMLInputElement | null>(null);
  let pageSizeLimitEl = $state<HTMLInputElement | null>(null);
  let searchPageSizeLimitEl = $state<HTMLInputElement | null>(null);
  let searchCacheSizeEl = $state<HTMLInputElement | null>(null);
  let loadToleranceEl = $state<HTMLInputElement | null>(null);

  $effect(() => {
    updateSliderTrack(transparencyEl);
  });

  $effect(() => {
    updateSliderTrack(viewerOpacityEl);
    updateSliderTrack(maxTextLinesEl);
    updateSliderTrack(pageSizeEl);
    updateSliderTrack(pageSizeLimitEl);
    updateSliderTrack(searchPageSizeLimitEl);
    updateSliderTrack(searchCacheSizeEl);
    updateSliderTrack(loadToleranceEl);
  });
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("general.eyebrow")}</span>
      <h2>{_t("general.title")}</h2>
      <p>{_t("general.description")}</p>
    </div>
    {#if s.showSettingsCloseButton}
      <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
        >×</button
      >
    {/if}
  </header>
{/if}

<div class="settings-scroll">
  {#if section === "search"}
    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="search" size={17} /></span>
        <div>
          <strong>{_t("general.searchSuggestionMode")}</strong>
          <p>{_t("general.searchSuggestionModeDescription")}</p>
        </div>
      </div>
      <select
        class="theme-select"
        value={s.searchSuggestionMode}
        aria-label={_t("general.searchSuggestionMode")}
        onchange={(e) =>
          generalSettings.updateSetting(
            "searchSuggestionMode",
            (e.target as HTMLSelectElement).value as SearchSuggestionMode,
          )}
      >
        <option value="off">{_t("general.searchSuggestionOff")}</option>
        <option value="panel">{_t("general.searchSuggestionPanel")}</option>
        <option value="inline">{_t("general.searchSuggestionInline")}</option>
      </select>
    </section>

    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
        <div>
          <strong>{_t("general.searchHistory")}</strong>
          <p>{_t("general.searchHistoryDescription")}</p>
        </div>
      </div>
      <button
        type="button"
        class="toggle-switch"
        class:active={s.searchHistoryEnabled}
        onclick={() =>
          generalSettings.updateSetting("searchHistoryEnabled", !s.searchHistoryEnabled)}
        aria-checked={s.searchHistoryEnabled}
        aria-label={_t("general.searchHistory")}
        role="switch"
      >
        <span class="toggle-knob"></span>
      </button>
    </section>

    <section class="setting-card sort-rules-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="sliders" size={17} /></span>
        <div>
          <strong>{_t("general.searchSortRules")}</strong>
          <p>{_t("general.searchSortRulesDescription")}</p>
        </div>
      </div>
      <div class="sort-rules-list" role="list" bind:this={sortListEl}>
        {#each s.searchSortRules as rule, idx (idx)}
          <div class="sort-rule-row" class:sort-dragging={sortDragIdx === idx} role="listitem">
            <span
              class="sort-grip"
              role="button"
              tabindex="0"
              aria-label={_t("general.sortDragHandle")}
              onpointerdown={(e) => pointerDragStart(idx, e)}
            >
              <span class="grip-dot"></span>
              <span class="grip-dot"></span>
              <span class="grip-dot"></span>
              <span class="grip-dot"></span>
            </span>
            <select
              class="theme-select sort-field-select"
              value={rule.field}
              aria-label={_t("general.searchSortRules")}
              onchange={(e) => {
                const newRules = [...s.searchSortRules];
                newRules[idx] = {
                  ...rule,
                  field: (e.target as HTMLSelectElement).value as SortRule["field"],
                };
                generalSettings.updateSetting("searchSortRules", newRules);
              }}
            >
              {#each ALL_SORT_FIELDS as f}
                {@const usedByOthers = s.searchSortRules.some(
                  (r: SortRule, i: number) => i !== idx && r.field === f,
                )}
                <option value={f} disabled={usedByOthers}>{_t(SORT_FIELD_LABELS[f])}</option>
              {/each}
            </select>
            <button
              type="button"
              class="sort-direction-btn"
              title={rule.direction === "asc" ? _t("general.sortAsc") : _t("general.sortDesc")}
              aria-label={rule.direction === "asc" ? _t("general.sortAsc") : _t("general.sortDesc")}
              onclick={() => {
                const newRules = [...s.searchSortRules];
                newRules[idx] = {
                  ...rule,
                  direction: rule.direction === "asc" ? "desc" : ("asc" as const),
                };
                generalSettings.updateSetting("searchSortRules", newRules);
              }}
            >
              {rule.direction === "asc" ? "↑" : "↓"}
            </button>
            {#if s.searchSortRules.length > 1}
              <button
                type="button"
                class="sort-remove-btn"
                title={_t("general.sortRemoveRule")}
                aria-label={_t("general.sortRemoveRule")}
                onclick={() => {
                  const newRules = s.searchSortRules.filter((_, i) => i !== idx);
                  generalSettings.updateSetting("searchSortRules", newRules);
                }}>×</button
              >
            {/if}
          </div>
        {/each}
      </div>
      {#if s.searchSortRules.length < 3}
        {#if s.searchSortRules.length < ALL_SORT_FIELDS.length}
          <button
            type="button"
            class="sort-add-btn"
            onclick={() => {
              const used = new Set(s.searchSortRules.map((r: SortRule) => r.field));
              const field = (ALL_SORT_FIELDS.find((f) => !used.has(f)) ??
                "createdAt") as SortRule["field"];
              const rule: SortRule = { field, direction: "desc" };
              generalSettings.updateSetting("searchSortRules", [...s.searchSortRules, rule]);
            }}
          >
            + {_t("general.sortAddRule")}
          </button>
        {/if}
      {/if}
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="search" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("general.searchPageSizeLimit")}</strong>
            <p>{_t("general.searchPageSizeLimitDescription")}</p>
          </div>
          <span class="value-label"
            >{s.searchPageSizeLimit} {_t("general.searchPageSizeLimitUnit")}</span
          >
        </div>
      </div>
      <input
        type="range"
        min="50"
        max="1000"
        step="50"
        value={s.searchPageSizeLimit}
        oninput={(event) => {
          const input = event.target as HTMLInputElement;
          generalSettings.updateSetting("searchPageSizeLimit", Number(input.value));
          updateSliderTrack(input);
        }}
        class="transparency-slider"
        bind:this={searchPageSizeLimitEl}
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="search" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("general.searchCacheSize")}</strong>
            <p>{_t("general.searchCacheSizeDescription")}</p>
          </div>
          <span class="value-label"
            >{s.searchCacheSize} {_t("general.searchCacheSizeUnit")}</span
          >
        </div>
      </div>
      <input
        type="range"
        min="200"
        max="2000"
        step="50"
        value={s.searchCacheSize}
        oninput={(event) => {
          const input = event.target as HTMLInputElement;
          generalSettings.updateSetting("searchCacheSize", Number(input.value));
          updateSliderTrack(input);
        }}
        class="transparency-slider"
        bind:this={searchCacheSizeEl}
      />
    </section>

    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="sliders" size={17} /></span>
        <div>
          <strong>{_t("general.searchCacheEviction")}</strong>
          <p>{_t("general.searchCacheEvictionDescription")}</p>
        </div>
      </div>
      <select
        class="theme-select"
        value={s.searchCacheEviction}
        aria-label={_t("general.searchCacheEviction")}
        onchange={(e) =>
          generalSettings.updateSetting(
            "searchCacheEviction",
            (e.target as HTMLSelectElement).value as "fifo" | "lru",
          )}
      >
        <option value="fifo">{_t("general.searchCacheEvictionFifo")}</option>
        <option value="lru">{_t("general.searchCacheEvictionLru")}</option>
      </select>
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="file" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("general.loadTolerance")}</strong>
            <p>{_t("general.loadToleranceDescription")}</p>
          </div>
          <span class="value-label"
            >{s.loadTolerance} {_t("general.loadToleranceUnit")}</span
          >
        </div>
      </div>
      <input
        type="range"
        min="50"
        max="500"
        step="50"
        value={s.loadTolerance}
        oninput={(event) => {
          const input = event.target as HTMLInputElement;
          generalSettings.updateSetting("loadTolerance", Number(input.value));
          updateSliderTrack(input);
        }}
        class="transparency-slider"
        bind:this={loadToleranceEl}
      />
    </section>
  {:else if section === "display"}
    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="eye" size={17} /></span>
        <div>
          <strong>{_t("general.showSecondaryText")}</strong>
          <p>{_t("general.showSecondaryTextDescription")}</p>
        </div>
      </div>
      <button
        type="button"
        class="toggle-switch"
        class:active={s.display.showSecondaryText}
        onclick={() =>
          generalSettings.updateSetting("display", {
            ...s.display,
            showSecondaryText: !s.display.showSecondaryText,
          })}
        aria-checked={s.display.showSecondaryText}
        aria-label={_t("general.showSecondaryText")}
        role="switch"
      >
        <span class="toggle-knob"></span>
      </button>
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="text" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("general.maxTextLines")}</strong>
            <p>{_t("general.maxTextLinesDescription")}</p>
          </div>
          <span class="value-label">{s.display.maxTextLines} {_t("general.maxTextLinesUnit")}</span>
        </div>
      </div>
      <input
        type="range"
        min="1"
        max="12"
        value={s.display.maxTextLines}
        oninput={(event) => {
          const input = event.target as HTMLInputElement;
          const val = Number(input.value);
          generalSettings.updateSetting("display", { ...s.display, maxTextLines: val });
          updateSliderTrack(maxTextLinesEl);
        }}
        class="transparency-slider"
        bind:this={maxTextLinesEl}
      />
    </section>

    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="grid" size={17} /></span>
        <div>
          <strong>{_t("general.detailDisplayMode")}</strong>
          <p>{_t("general.detailDisplayModeDescription")}</p>
        </div>
      </div>
      <select
        class="theme-select"
        value={s.detailDisplayMode}
        aria-label={_t("general.detailDisplayMode")}
        onchange={(e) =>
          generalSettings.updateSetting(
            "detailDisplayMode",
            (e.target as HTMLSelectElement).value as "overlay" | "split",
          )}
      >
        <option value="overlay">{_t("general.detailDisplayModeOverlay")}</option>
        <option value="split">{_t("general.detailDisplayModeSplit")}</option>
      </select>
    </section>

    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="maximize" size={17} /></span>
        <div>
          <strong>{_t("general.desktopFullscreen")}</strong>
          <p>{_t("general.desktopFullscreenDescription")}</p>
        </div>
      </div>
      <button
        type="button"
        class="toggle-switch"
        class:active={s.imageFullscreenMode === "desktop"}
        onclick={() =>
          generalSettings.updateSetting(
            "imageFullscreenMode",
            s.imageFullscreenMode === "desktop" ? "overlay" : "desktop",
          )}
        aria-checked={s.imageFullscreenMode === "desktop"}
        aria-label={_t("general.desktopFullscreen")}
        role="switch"
      >
        <span class="toggle-knob"></span>
      </button>
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="image" size={17} /></span>
        <div class="heading-inline">
          <strong>{_t("general.viewerBackdropOpacity")}</strong>
          <span class="value-label">{s.viewerBackdropOpacity}%</span>
        </div>
      </div>
      <input
        type="range"
        min="0"
        max="100"
        value={s.viewerBackdropOpacity}
        oninput={(e) => {
          generalSettings.updateSetting(
            "viewerBackdropOpacity",
            Number((e.target as HTMLInputElement).value),
          );
          updateSliderTrack(viewerOpacityEl);
        }}
        class="transparency-slider"
        bind:this={viewerOpacityEl}
      />
    </section>
  {:else if section === "items"}
    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="grid" size={17} /></span>
        <div>
          <strong>{_t("general.cardActionsDisplay")}</strong>
          <p>{_t("general.cardActionsDisplayDescription")}</p>
        </div>
      </div>
      <select
        class="theme-select"
        value={s.cardActionsDisplay}
        aria-label={_t("general.cardActionsDisplay")}
        onchange={(e) =>
          generalSettings.updateSetting(
            "cardActionsDisplay",
            (e.target as HTMLSelectElement).value as CardActionsDisplay,
          )}
      >
        <option value="hover">{_t("general.cardActionsHover")}</option>
        <option value="always">{_t("general.cardActionsAlways")}</option>
      </select>
    </section>

    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="copy" size={17} /></span>
        <div>
          <strong>{_t("general.quickCopyBadge")}</strong>
          <p>{_t("general.quickCopyBadgeDescription")}</p>
        </div>
      </div>
      <button
        type="button"
        class="toggle-switch"
        class:active={s.quickCopyBadgeAlwaysVisible}
        onclick={() =>
          generalSettings.updateSetting(
            "quickCopyBadgeAlwaysVisible",
            !s.quickCopyBadgeAlwaysVisible,
          )}
        aria-checked={s.quickCopyBadgeAlwaysVisible}
        aria-label={_t("general.quickCopyBadge")}
        role="switch"
      >
        <span class="toggle-knob"></span>
      </button>
    </section>

    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="grid" size={17} /></span>
        <div>
          <strong>{_t("general.pinCopiedToTop")}</strong>
          <p>{_t("general.pinCopiedToTopDescription")}</p>
        </div>
      </div>
      <button
        type="button"
        class="toggle-switch"
        class:active={s.pinCopiedToTop}
        onclick={() => generalSettings.updateSetting("pinCopiedToTop", !s.pinCopiedToTop)}
        aria-checked={s.pinCopiedToTop}
        aria-label={_t("general.pinCopiedToTop")}
        role="switch"
      >
        <span class="toggle-knob"></span>
      </button>
    </section>

    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="trash" size={17} /></span>
        <div>
          <strong>{_t("general.useRecycleBin")}</strong>
          <p>{_t("general.useRecycleBinDescription")}</p>
        </div>
      </div>
      <button
        type="button"
        class="toggle-switch"
        class:active={s.useRecycleBin}
        onclick={() => generalSettings.updateSetting("useRecycleBin", !s.useRecycleBin)}
        aria-checked={s.useRecycleBin}
        aria-label={_t("general.useRecycleBin")}
        role="switch"
      >
        <span class="toggle-knob"></span>
      </button>
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="file" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("general.pageSize")}</strong>
            <p>{_t("general.pageSizeDescription")}</p>
          </div>
          <span class="value-label">{s.display.pageSize} {_t("general.pageSizeUnit")}</span>
        </div>
      </div>
      <input
        type="range"
        min="50"
        max={Math.min(s.pageSizeLimit, 300)}
        step="50"
        value={Math.min(s.display.pageSize, s.pageSizeLimit)}
        oninput={(event) => {
          const input = event.target as HTMLInputElement;
          const val = Math.min(Number(input.value), s.pageSizeLimit);
          generalSettings.updateSetting("display", {
            ...s.display,
            pageSize: val,
          });
          updateSliderTrack(input);
        }}
        class="transparency-slider"
        bind:this={pageSizeEl}
      />
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="file" size={17} /></span>
        <div class="heading-inline">
          <div>
            <strong>{_t("general.pageSizeLimit")}</strong>
            <p>{_t("general.pageSizeLimitDescription")}</p>
          </div>
          <span class="value-label"
            >{s.pageSizeLimit} {_t("general.pageSizeLimitUnit")}</span
          >
        </div>
      </div>
      <input
        type="range"
        min="500"
        max="6000"
        step="100"
        value={s.pageSizeLimit}
        oninput={(event) => {
          const input = event.target as HTMLInputElement;
          const val = Number(input.value);
          generalSettings.updateSetting("pageSizeLimit", val);
          if (s.display.pageSize > val) {
            generalSettings.updateSetting("display", { ...s.display, pageSize: val });
          }
          updateSliderTrack(input);
          requestAnimationFrame(() => updateSliderTrack(pageSizeEl));
        }}
        class="transparency-slider"
        bind:this={pageSizeLimitEl}
      />
    </section>
  {:else}
    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="globe" size={17} /></span>
        <div>
          <strong>{_t("general.language")}</strong>
          <p>{_t("general.languageDescription")}</p>
        </div>
      </div>
      <div class="lang-toggle">
        <button
          type="button"
          class:active={s.language === "zh-CN"}
          onclick={() => changeLanguage("zh-CN")}>中文</button
        >
        <button
          type="button"
          class:active={s.language === "en"}
          onclick={() => changeLanguage("en")}>English</button
        >
      </div>
    </section>

    <section class="setting-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="sliders" size={17} /></span>
        <div class="heading-inline">
          <strong>{_t("general.windowTransparency")}</strong>
          <span class="value-label">{s.windowTransparency}%</span>
        </div>
      </div>
      <input
        type="range"
        min="60"
        max="100"
        value={s.windowTransparency}
        oninput={handleTransparency}
        class="transparency-slider"
        bind:this={transparencyEl}
      />
    </section>

    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="clock" size={17} /></span>
        <div>
          <strong>{_t("general.launchAtStartup")}</strong>
          <p>{_t("general.launchAtStartupDescription")}</p>
        </div>
      </div>
      <button
        type="button"
        class="toggle-switch"
        class:active={windowConfig?.launchAtStartup ?? false}
        onclick={() =>
          void changeWindowSetting("launchAtStartup", !(windowConfig?.launchAtStartup ?? false))}
        disabled={windowConfigLoading || windowConfigSaving || !windowConfig}
        aria-checked={windowConfig?.launchAtStartup ?? false}
        aria-label={_t("general.launchAtStartup")}
        role="switch"
      >
        <span class="toggle-knob"></span>
      </button>
    </section>

    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="clipboard" size={17} /></span>
        <div>
          <strong>{_t("general.closeToTray")}</strong>
          <p>{_t("general.closeToTrayDescription")}</p>
        </div>
      </div>
      <button
        type="button"
        class="toggle-switch"
        class:active={windowConfig?.closeToTray ?? false}
        onclick={() =>
          void changeWindowSetting("closeToTray", !(windowConfig?.closeToTray ?? false))}
        disabled={windowConfigLoading || windowConfigSaving || !windowConfig}
        aria-checked={windowConfig?.closeToTray ?? false}
        aria-label={_t("general.closeToTray")}
        role="switch"
      >
        <span class="toggle-knob"></span>
      </button>
    </section>

    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="copy" size={17} /></span>
        <div>
          <strong>{_t("general.useSystemTitleBar")}</strong>
          <p>{_t("general.useSystemTitleBarDescription")}</p>
        </div>
      </div>
      <button
        type="button"
        class="toggle-switch"
        class:active={s.useSystemTitleBar}
        onclick={() => generalSettings.updateSetting("useSystemTitleBar", !s.useSystemTitleBar)}
        aria-checked={s.useSystemTitleBar}
        aria-label={_t("general.useSystemTitleBar")}
        role="switch"
      >
        <span class="toggle-knob"></span>
      </button>
    </section>

    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="x" size={17} /></span>
        <div>
          <strong>{_t("general.showSettingsCloseButton")}</strong>
          <p>{_t("general.showSettingsCloseButtonDescription")}</p>
        </div>
      </div>
      <button
        type="button"
        class="toggle-switch"
        class:active={s.showSettingsCloseButton}
        onclick={() =>
          generalSettings.updateSetting("showSettingsCloseButton", !s.showSettingsCloseButton)}
        aria-checked={s.showSettingsCloseButton}
        aria-label={_t("general.showSettingsCloseButton")}
        role="switch"
      >
        <span class="toggle-knob"></span>
      </button>
    </section>

    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="pin" size={17} /></span>
        <div>
          <strong>{_t("general.rememberWindowPosition")}</strong>
          <p>{_t("general.rememberWindowPositionDescription")}</p>
        </div>
      </div>
      <button
        type="button"
        class="toggle-switch"
        class:active={s.rememberWindowPosition}
        onclick={() =>
          generalSettings.updateSetting("rememberWindowPosition", !s.rememberWindowPosition)}
        aria-checked={s.rememberWindowPosition}
        aria-label={_t("general.rememberWindowPosition")}
        role="switch"
      >
        <span class="toggle-knob"></span>
      </button>
    </section>

    <section class="setting-card toggle-card">
      <div class="setting-heading">
        <span class="setting-icon"><AppIcon name="info" size={17} /></span>
        <div>
          <strong>{_t("general.toastNotifications")}</strong>
          <p>{_t("general.toastNotificationsDescription")}</p>
        </div>
      </div>
      <button
        type="button"
        class="toggle-switch"
        class:active={s.showToastNotifications}
        onclick={() =>
          generalSettings.updateSetting("showToastNotifications", !s.showToastNotifications)}
        aria-checked={s.showToastNotifications}
        aria-label={_t("general.toastNotifications")}
        role="switch"
      >
        <span class="toggle-knob"></span>
      </button>
    </section>
  {/if}

  <p class="auto-save-note">{_t("general.autoSaveNote")}</p>
</div>

{#if feedback}
  <div class:success={feedbackSuccess} class="settings-feedback">{feedback}</div>
{/if}

<style>
  .lang-toggle {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }

  .lang-toggle button {
    padding: 7px 16px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-muted);
    background: var(--input-bg);
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    transition:
      background 100ms ease,
      border-color 100ms ease,
      color 100ms ease;
  }

  .lang-toggle button:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .lang-toggle button.active {
    border-color: var(--selection-color);
    color: var(--text-primary);
    background: color-mix(in srgb, var(--selection-color) 15%, transparent);
  }

  .theme-select {
    padding: 5px 8px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-primary);
    background: var(--input-bg);
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    flex-shrink: 0;
    outline: none;
    appearance: none;
    -webkit-appearance: none;
    -moz-appearance: none;
  }

  .theme-select:focus {
    border-color: var(--text-faint);
  }

  .sort-rules-card {
    flex-direction: column;
    align-items: stretch;
  }

  .sort-rules-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 8px 0;
  }

  .sort-rule-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
    border-radius: var(--settings-control-radius, 6px);
  }

  .sort-rule-row.sort-dragging {
    opacity: 0.4;
  }

  :global(.sort-drag-over) {
    outline: 2px solid var(--accent) !important;
    outline-offset: 2px;
    border-radius: 6px;
    background: rgba(74, 168, 255, 0.15);
  }

  .sort-grip {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
    padding: 4px;
    cursor: grab;
    flex-shrink: 0;
    border-radius: 4px;
    align-self: stretch;
    align-content: center;
    user-select: none;
    touch-action: none;
  }

  .sort-grip:hover {
    background: var(--hover-bg);
  }

  .sort-grip:active {
    cursor: grabbing;
  }

  .grip-dot {
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--text-muted);
    display: block;
  }

  .sort-field-select {
    flex: 1;
    min-width: 0;
  }

  .sort-direction-btn {
    width: 32px;
    height: 28px;
    border-radius: var(--settings-control-radius, 6px);
    border: 1px solid var(--border-color);
    background: var(--input-bg);
    color: var(--text-primary);
    font-size: 14px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .sort-direction-btn:hover {
    background: var(--hover-bg);
  }

  .sort-remove-btn {
    width: 28px;
    height: 28px;
    border-radius: var(--settings-control-radius, 6px);
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 16px;
    flex-shrink: 0;
  }

  .sort-remove-btn:hover {
    color: var(--danger-color);
    border-color: var(--danger-color);
  }

  .sort-add-btn {
    margin-top: 4px;
    padding: 6px 12px;
    border-radius: var(--settings-control-radius, 6px);
    border: 1px dashed var(--border-color);
    background: transparent;
    color: var(--text-muted);
    font-size: var(--settings-control-size, 11px);
    cursor: pointer;
    align-self: flex-start;
  }

  .sort-add-btn:hover {
    border-color: var(--text-muted);
    color: var(--text-primary);
  }
</style>
