// Shared focus-trap helpers for modal dialogs (WCAG 2.4.3 / ARIA dialog
// pattern). Keep behavior identical across DetailPanel and settings dialogs.

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  'button[disabled="false"]',
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(", ");

/** Returns currently visible, keyboard-focusable elements inside `container`. */
export function getFocusableElements(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (element) => {
      if (element.getAttribute("aria-hidden") === "true") return false;
      // Browsers expose a layout-aware visibility probe; environments without
      // one (jsdom) fall back to inline styles.
      if (typeof element.checkVisibility === "function") return element.checkVisibility();
      return element.style.display !== "none";
    },
  );
}

/**
 * Wraps Tab focus inside `container`. Attach as the dialog element's
 * `onkeydowncapture` handler. Returns without acting when the event is not a
 * Tab so other handlers keep working.
 */
export function trapTabFocus(container: HTMLElement | null, event: KeyboardEvent): void {
  if (event.key !== "Tab" || !container) return;

  const focusable = getFocusableElements(container);
  if (focusable.length === 0) return;

  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  const active = document.activeElement;
  const inside = active instanceof Node && container.contains(active);

  if (!inside) {
    event.preventDefault();
    first.focus();
    return;
  }

  if (event.shiftKey && active === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && active === last) {
    event.preventDefault();
    first.focus();
  }
}

/** Remembers the currently focused element and returns a restore function. */
export function captureFocusRestore(): () => void {
  const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  return () => {
    previous?.focus();
  };
}
