<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import { generalSettings } from "$lib/services/settings";
  import type { FloatPanelClickAction } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    showHeader?: boolean;
  }

  let { showHeader = true }: Props = $props();

  let s = $state($generalSettings);

  $effect(() => {
    const unsubscribe = generalSettings.subscribe((value) => {
      s = value;
    });
    return unsubscribe;
  });

  const OPTIONS: FloatPanelClickAction[] = [
    "none",
    "copy",
    "copyPaste",
    "favorite",
    "detail",
    "delete",
  ];

  function optionLabel(action: FloatPanelClickAction): string {
    return _t(`floatClick.${action}`);
  }

  function selectOptions(
    except: "floatPanelLeftClick" | "floatPanelRightClick" | "floatPanelMiddleClick",
  ) {
    const used = new Set<FloatPanelClickAction>();
    if (except !== "floatPanelLeftClick" && s.floatPanelLeftClick !== "none")
      used.add(s.floatPanelLeftClick);
    if (except !== "floatPanelRightClick" && s.floatPanelRightClick !== "none")
      used.add(s.floatPanelRightClick);
    if (except !== "floatPanelMiddleClick" && s.floatPanelMiddleClick !== "none")
      used.add(s.floatPanelMiddleClick);
    return OPTIONS.map((value) => ({
      value,
      label: optionLabel(value),
      disabled: used.has(value),
    }));
  }

  function change(
    key: "floatPanelLeftClick" | "floatPanelRightClick" | "floatPanelMiddleClick",
    value: string | number,
  ) {
    generalSettings.updateSetting(key, String(value) as FloatPanelClickAction);
  }
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("keyboard.settings")}</span>
      <h2>{_t("floatClick.title")}</h2>
      <p>{_t("floatClick.description")}</p>
    </div>
  </header>
{/if}

<div class="settings-scroll">
  <section class="setting-card toggle-card" data-settings-search-id="keyboard.float-click-left">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="clipboard" size={17} /></span>
      <div>
        <strong>{_t("floatClick.leftClick")}</strong>
        <p>{_t("floatClick.leftClickDesc")}</p>
      </div>
    </div>
    <CustomSelect
      value={s.floatPanelLeftClick}
      options={selectOptions("floatPanelLeftClick")}
      onchange={(v) => change("floatPanelLeftClick", v)}
    />
  </section>

  <section class="setting-card toggle-card" data-settings-search-id="keyboard.float-click-right">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="clipboard" size={17} /></span>
      <div>
        <strong>{_t("floatClick.rightClick")}</strong>
        <p>{_t("floatClick.rightClickDesc")}</p>
      </div>
    </div>
    <CustomSelect
      value={s.floatPanelRightClick}
      options={selectOptions("floatPanelRightClick")}
      onchange={(v) => change("floatPanelRightClick", v)}
    />
  </section>

  <section class="setting-card toggle-card" data-settings-search-id="keyboard.float-click-middle">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="clipboard" size={17} /></span>
      <div>
        <strong>{_t("floatClick.middleClick")}</strong>
        <p>{_t("floatClick.middleClickDesc")}</p>
      </div>
    </div>
    <CustomSelect
      value={s.floatPanelMiddleClick}
      options={selectOptions("floatPanelMiddleClick")}
      onchange={(v) => change("floatPanelMiddleClick", v)}
    />
  </section>
</div>
