<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { locale, messages, resolvePath } from "$lib/i18n";

  interface Props {
    value: string;
    onchange: (value: string) => void;
    ariaLabel?: string;
    disabled?: boolean;
  }

  let { value, onchange, ariaLabel, disabled = false }: Props = $props();

  const _t = (path: string) => resolvePath($messages, path);

  const weekStart = $derived($locale === "zh-CN" ? 1 : 0);
  const displayFmt = $derived(
    new Intl.DateTimeFormat($locale, { year: "numeric", month: "2-digit", day: "2-digit" }),
  );
  const monthFmt = $derived(new Intl.DateTimeFormat($locale, { year: "numeric", month: "long" }));
  const weekdayFmt = $derived(
    new Intl.DateTimeFormat($locale, { weekday: $locale === "zh-CN" ? "narrow" : "short" }),
  );

  let open = $state(false);
  let viewYear = $state(0);
  let viewMonth = $state(0);
  let popoverTop = $state(0);
  let popoverLeft = $state(0);
  let triggerEl: HTMLButtonElement | undefined = $state();
  let popoverEl: HTMLDivElement | undefined = $state();

  function toIso(d: Date): string {
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, "0");
    const day = String(d.getDate()).padStart(2, "0");
    return `${y}-${m}-${day}`;
  }

  function parseDate(iso: string): Date | null {
    const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso);
    if (!match) return null;
    return new Date(+match[1], +match[2] - 1, +match[3]);
  }

  const selectedDate = $derived(parseDate(value));
  const today = $derived(new Date());
  const viewLabel = $derived(monthFmt.format(new Date(viewYear, viewMonth, 1)));

  const weekdayNames = $derived(
    Array.from({ length: 7 }, (_, column) => {
      const offset = (column + weekStart) % 7;
      return weekdayFmt.format(new Date(2021, 0, 4 + offset));
    }),
  );

  const days = $derived.by(() => {
    const first = new Date(viewYear, viewMonth, 1);
    const firstColumn = (first.getDay() - weekStart + 7) % 7;
    const start = new Date(viewYear, viewMonth, 1 - firstColumn);
    const todayIso = toIso(today);
    return Array.from({ length: 42 }, (_, i) => {
      const d = new Date(start.getFullYear(), start.getMonth(), start.getDate() + i);
      const iso = toIso(d);
      return {
        date: d,
        iso,
        day: d.getDate(),
        inMonth: d.getMonth() === viewMonth,
        today: iso === todayIso,
        selected: value === iso,
      };
    });
  });

  function positionPopover() {
    if (!open || !triggerEl || !popoverEl) return;
    const rect = triggerEl.getBoundingClientRect();
    const popHeight = popoverEl.offsetHeight;
    const gap = 4;
    const topBelow = rect.bottom + gap;
    const topAbove = rect.top - gap - popHeight;
    const fitsBelow = topBelow + popHeight <= window.innerHeight - 8;
    const fitsAbove = topAbove >= 8;
    popoverTop = fitsBelow || !fitsAbove ? topBelow : topAbove;
    const popWidth = popoverEl.offsetWidth;
    popoverLeft = Math.max(8, Math.min(rect.right - popWidth, window.innerWidth - 8 - popWidth));
  }

  function toggle() {
    if (disabled) return;
    if (!open) {
      const anchor = parseDate(value) ?? today;
      viewYear = anchor.getFullYear();
      viewMonth = anchor.getMonth();
    }
    open = !open;
  }

  $effect(() => {
    if (!open) return;
    positionPopover();
    window.addEventListener("resize", positionPopover);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      window.removeEventListener("resize", positionPopover);
      window.removeEventListener("scroll", onScroll, true);
    };
  });

  function onScroll(e: Event) {
    if (popoverEl && e.target instanceof Node && popoverEl.contains(e.target)) return;
    open = false;
  }

  function select(iso: string) {
    open = false;
    onchange(iso);
  }

  function clear() {
    open = false;
    onchange("");
  }

  function shiftMonth(delta: number) {
    const month = viewMonth + delta;
    viewYear = viewYear + Math.floor(month / 12) + (month < 0 && month % 12 !== 0 ? -1 : 0);
    viewMonth = ((month % 12) + 12) % 12;
  }
</script>

<div class="date-picker">
  <button
    type="button"
    class="date-picker-trigger"
    class:open
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-label={ariaLabel}
    {disabled}
    bind:this={triggerEl}
    onclick={toggle}
    onkeydown={(e) => {
      if (e.key === "Escape") open = false;
    }}
  >
    <span class:placeholder={!value}>
      {value && selectedDate
        ? displayFmt.format(selectedDate)
        : _t("storage.datePickerPlaceholder")}
    </span>
    <AppIcon name="calendar" size={14} />
  </button>

  {#if open}
    <div
      class="date-picker-popover popover-surface"
      role="dialog"
      aria-label={ariaLabel}
      style="top: {popoverTop}px; left: {popoverLeft}px;"
      bind:this={popoverEl}
    >
      <div class="date-picker-backdrop" onclick={() => (open = false)} aria-hidden="true"></div>
      <div class="date-picker-header">
        <button
          type="button"
          class="date-nav"
          aria-label={_t("storage.datePickerPrevMonth")}
          onclick={() => shiftMonth(-1)}
        >
          <AppIcon name="chevron-left" size={14} />
        </button>
        <span class="date-view-label">{viewLabel}</span>
        <button
          type="button"
          class="date-nav"
          aria-label={_t("storage.datePickerNextMonth")}
          onclick={() => shiftMonth(1)}
        >
          <AppIcon name="chevron-right" size={14} />
        </button>
      </div>
      <div class="date-weekdays">
        {#each weekdayNames as name (name)}
          <span>{name}</span>
        {/each}
      </div>
      <div class="date-grid">
        {#each days as day (day.iso)}
          <button
            type="button"
            class:outside={!day.inMonth}
            class:today={day.today}
            class:selected={day.selected}
            aria-pressed={day.selected}
            onclick={() => select(day.iso)}
          >
            {day.day}
          </button>
        {/each}
      </div>
      <div class="date-picker-footer">
        <button type="button" class="date-footer-btn" onclick={() => select(toIso(today))}>
          {_t("storage.datePickerToday")}
        </button>
        {#if value}
          <button type="button" class="date-footer-btn" onclick={clear}>
            {_t("storage.datePickerClear")}
          </button>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .date-picker {
    flex: 1 1 0;
    min-width: 0;
  }

  .date-picker-trigger {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    height: 30px;
    box-sizing: border-box;
    padding: 0 8px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    background: var(--input-bg);
    color: var(--text-primary);
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    cursor: pointer;
    transition: border-color 120ms ease;
  }

  .date-picker-trigger:hover,
  .date-picker-trigger.open {
    border-color: var(--text-faint);
  }

  .date-picker-trigger:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .date-picker-trigger > :global(svg) {
    flex-shrink: 0;
    color: var(--text-faint);
  }

  .date-picker-trigger > span {
    flex: 1;
    min-width: 0;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .date-picker-trigger > span.placeholder {
    color: var(--placeholder-color);
  }

  .date-picker-popover {
    position: fixed;
    width: 264px;
    box-sizing: border-box;
    padding: 10px;
  }

  .date-picker-backdrop {
    position: fixed;
    inset: 0;
    z-index: -1;
  }

  .date-picker-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  .date-view-label {
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    font-weight: 600;
    color: var(--text-primary);
  }

  .date-picker-header button.date-nav {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border-radius: var(--settings-control-radius, 6px);
  }

  button.date-nav :global(svg) {
    color: var(--text-faint);
  }

  .date-weekdays {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    margin-bottom: 2px;
  }

  .date-weekdays span {
    text-align: center;
    line-height: 22px;
    font-size: 10px;
    color: var(--text-faint);
  }

  .date-grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 2px;
  }

  .date-grid button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 30px;
    box-sizing: border-box;
    padding: 0;
    border-radius: var(--settings-control-radius, 6px);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
  }

  .date-grid button.outside {
    color: var(--text-faint);
    opacity: 0.55;
  }

  .date-grid button.today {
    border: 1px solid var(--border-color);
  }

  .date-grid button.selected,
  .date-grid button.selected:hover {
    color: #fff;
    background: var(--selection-color);
  }

  .date-picker-footer {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 8px;
    padding-top: 8px;
    border-top: 1px solid var(--border-subtle);
  }

  .date-picker-footer button.date-footer-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    width: auto;
    padding: 4px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    background: var(--hover-bg);
  }
</style>
