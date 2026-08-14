const ELLIPSIS = "\u2026";

type PopoverAnchorRect = Pick<DOMRect, "bottom" | "left" | "right" | "top">;

interface FixedPopoverPositionOptions {
  align?: "start" | "end";
  gap?: number;
  viewportPadding?: number;
  viewportWidth?: number;
  viewportHeight?: number;
}

export interface FixedPopoverPosition {
  top: number;
  left: number;
}

export function resolveFixedPopoverPosition(
  anchor: PopoverAnchorRect,
  popoverWidth: number,
  popoverHeight: number,
  options: FixedPopoverPositionOptions = {},
): FixedPopoverPosition {
  const {
    align = "start",
    gap = 4,
    viewportPadding = 8,
    viewportWidth = window.innerWidth,
    viewportHeight = window.innerHeight,
  } = options;
  const topBelow = anchor.bottom + gap;
  const topAbove = anchor.top - gap - popoverHeight;
  const fitsBelow = topBelow + popoverHeight <= viewportHeight - viewportPadding;
  const fitsAbove = topAbove >= viewportPadding;
  const desiredLeft = align === "end" ? anchor.right - popoverWidth : anchor.left;
  const maxLeft = viewportWidth - viewportPadding - popoverWidth;

  return {
    top: fitsBelow || !fitsAbove ? topBelow : topAbove,
    left: Math.max(viewportPadding, Math.min(desiredLeft, maxLeft)),
  };
}

export function alignDropdownOptionText(container: HTMLElement): void {
  let anyOverflow = false;
  for (const button of container.querySelectorAll<HTMLElement>("button")) {
    const probe = button.querySelector<HTMLElement>(":scope > span") ?? button;
    if (probe.clientWidth > 0 && probe.scrollWidth > probe.clientWidth + 1) {
      anyOverflow = true;
      truncateToFit(button, probe);
    }
  }
  container.classList.toggle("text-overflow", anyOverflow);
}

function truncateToFit(button: HTMLElement, probe: HTMLElement): void {
  // Capture the original label once; reuse it on later runs so re-measuring the
  // already-truncated text (e.g. while filtering the source-app list) does not
  // shrink the label further on every keystroke.
  const original = probe.dataset.fullLabel ?? (probe.dataset.fullLabel = probe.textContent ?? "");
  const full = original;
  const chars = Array.from(full);
  if (chars.length <= 1) return;
  button.title = full;
  probe.textContent = full + ELLIPSIS;
  let keep = chars.length;
  while (probe.scrollWidth > probe.clientWidth + 1 && keep > 1) {
    keep -= 1;
    probe.textContent = chars.slice(0, keep).join("") + ELLIPSIS;
  }
}
