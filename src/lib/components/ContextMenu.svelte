<script lang="ts">
  import { tick } from "svelte";
  import type { IconName } from "$lib/types/clipboard";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  export interface ContextMenuItem {
    id: string;
    label: string;
    icon: IconName;
    destructive?: boolean;
    disabled?: boolean;
    children?: ContextMenuItem[];
  }

  interface Props {
    x: number;
    y: number;
    items: ContextMenuItem[];
    onclose: () => void;
    onaction: (id: string) => void;
  }

  let { x, y, items, onclose, onaction }: Props = $props();

  let menuEl = $state<HTMLDivElement>();
  let posX = $state(0);
  let posY = $state(0);

  let activeSub = $state<string | null>(null);

  function adjustPosition(width: number, height: number) {
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    posX = Math.min(x, vw - width - 8);
    posY = Math.min(y, vh - height - 8);
  }

  $effect(() => {
    if (menuEl) {
      const rect = menuEl.getBoundingClientRect();
      adjustPosition(rect.width, rect.height);
    }
  });

  function openSub(id: string) {
    activeSub = id;
  }

  function closeSub(id: string) {
    if (activeSub === id) activeSub = null;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }

  // Keyboard support for submenu parents (they are plain divs, not buttons):
  // Enter/Space/ArrowRight expand the submenu, ArrowLeft collapses it.
  // (Escape intentionally keeps closing the entire context menu.)
  function handleParentKeydown(e: KeyboardEvent, id: string) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      openSub(id);
      void tick().then(() => {
        const first = menuEl?.querySelector<HTMLElement>(
          '.submenu[role="menu"] button:not([disabled])',
        );
        first?.focus();
      });
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      openSub(id);
      void tick().then(() => {
        const first = menuEl?.querySelector<HTMLElement>(
          '.submenu[role="menu"] button:not([disabled])',
        );
        first?.focus();
      });
    } else if (e.key === "ArrowLeft") {
      closeSub(id);
    }
  }

  function handleClickOutside(e: MouseEvent) {
    if (menuEl && !menuEl.contains(e.target as Node)) {
      onclose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} onclick={handleClickOutside} />

<div
  class="context-menu"
  bind:this={menuEl}
  style:left="{posX || x}px"
  style:top="{posY || y}px"
  role="menu"
  aria-label={_t("actions.contextMenu")}
>
  {#each items as item}
    {#if item.children?.length}
      <div
        class="menu-item menu-item-parent"
        role="menuitem"
        tabindex="0"
        aria-haspopup="menu"
        aria-expanded={activeSub === item.id}
        onmouseenter={() => openSub(item.id)}
        onmouseleave={() => closeSub(item.id)}
        onkeydown={(e) => handleParentKeydown(e, item.id)}
      >
        <span class="menu-icon"><AppIcon name={item.icon} size={15} /></span>
        <span class="menu-label">{item.label}</span>
        <span class="menu-chevron"><AppIcon name="chevron-right" size={13} /></span>
        {#if activeSub === item.id}
          <div class="submenu" role="menu">
            {#each item.children as child}
              <button
                type="button"
                class="menu-item"
                class:destructive={child.destructive}
                role="menuitem"
                disabled={child.disabled}
                onclick={() => {
                  onaction(child.id);
                  onclose();
                }}
              >
                <span class="menu-icon"><AppIcon name={child.icon} size={15} /></span>
                <span class="menu-label">{child.label}</span>
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <button
        type="button"
        class="menu-item"
        class:destructive={item.destructive}
        role="menuitem"
        disabled={item.disabled}
        onmouseenter={() => (activeSub = null)}
        onclick={() => {
          onaction(item.id);
          onclose();
        }}
      >
        <span class="menu-icon"><AppIcon name={item.icon} size={15} /></span>
        <span class="menu-label">{item.label}</span>
      </button>
    {/if}
  {/each}
</div>

<style>
  .context-menu {
    position: fixed;
    z-index: 9999;
    min-width: 160px;
    background: var(--surface-bg);
    border: 1px solid var(--border-color);
    border-radius: 8px;
    padding: 4px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(12px);
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 10px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--text-primary);
    font-size: 12.5px;
    cursor: pointer;
    text-align: left;
    transition: background 80ms ease;
  }

  .menu-item-parent {
    position: relative;
    box-sizing: border-box;
  }

  .menu-item-parent::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 100%;
    width: 10px;
  }

  .menu-item:hover {
    background: var(--hover-bg);
  }

  .menu-item:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .menu-item:disabled:hover {
    background: transparent;
  }

  .menu-item.destructive {
    color: var(--danger-color);
  }

  .menu-item.destructive:hover {
    background: color-mix(in srgb, var(--danger-color) 12%, transparent);
  }

  .menu-icon {
    display: inline-flex;
    align-items: center;
    width: 18px;
    flex-shrink: 0;
  }

  .menu-label {
    flex: 1;
  }

  .menu-chevron {
    display: inline-flex;
    align-items: center;
    color: var(--text-muted);
  }

  .submenu {
    position: absolute;
    top: 0;
    left: calc(100% + 8px);
    z-index: 10000;
    min-width: 180px;
    background: var(--surface-bg);
    border: 1px solid var(--border-color);
    border-radius: 8px;
    padding: 4px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(12px);
  }
</style>
