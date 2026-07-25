<script lang="ts">
  import type { IconName } from "$lib/components/AppIcon.svelte";
  import AppIcon from "$lib/components/AppIcon.svelte";

  export interface ContextMenuItem {
    id: string;
    label: string;
    icon: IconName;
    destructive?: boolean;
    disabled?: boolean;
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

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
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
  aria-label="上下文菜单"
>
  {#each items as item}
    <button
      type="button"
      class="menu-item"
      class:destructive={item.destructive}
      role="menuitem"
      disabled={item.disabled}
      onclick={() => { onaction(item.id); onclose(); }}
    >
      <span class="menu-icon"><AppIcon name={item.icon} size={15} /></span>
      <span class="menu-label">{item.label}</span>
    </button>
  {/each}
</div>

<style>
  .context-menu {
    position: fixed;
    z-index: 9999;
    min-width: 160px;
    background: #1e1e1e;
    border: 1px solid #3a3a3a;
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
    color: #d8d8d8;
    font-size: 12.5px;
    cursor: pointer;
    text-align: left;
    transition: background 80ms ease;
  }

  .menu-item:hover {
    background: #2a2a2a;
  }

  .menu-item:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .menu-item:disabled:hover {
    background: transparent;
  }

  .menu-item.destructive {
    color: #e85d5d;
  }

  .menu-item.destructive:hover {
    background: rgba(232, 93, 93, 0.12);
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
</style>
