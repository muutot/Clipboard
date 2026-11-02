// Clears the main-window search box N seconds after the window loses focus.
// Hiding a focused window also fires the focus-lost event, so one blur signal
// covers both "hidden" and "unfocused".

export interface SearchAutoClearOptions {
  /** Whether the feature is enabled (read live from settings). */
  enabled(): boolean;
  /** Delay in seconds between losing focus and clearing. */
  delaySeconds(): number;
  /** Whether there is a query worth clearing. */
  hasQuery(): boolean;
  /** Invoked when the delay elapses while the window is still unfocused. */
  onClear(): void;
}

export interface SearchAutoClear {
  handleWindowFocus(focused: boolean): void;
  dispose(): void;
}

export function createSearchAutoClear(options: SearchAutoClearOptions): SearchAutoClear {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let focused = true;

  function cancel() {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
  }

  return {
    handleWindowFocus(nextFocused: boolean) {
      focused = nextFocused;
      cancel();
      // Regained focus or nothing to clear: no timer.
      if (focused || !options.enabled() || !options.hasQuery()) return;
      const seconds = Math.max(1, Math.floor(options.delaySeconds()));
      timer = setTimeout(() => {
        timer = undefined;
        // The window may have regained focus while the timer was pending;
        // re-check at fire time instead of capturing the decision early.
        if (focused || !options.enabled() || !options.hasQuery()) return;
        options.onClear();
      }, seconds * 1000);
    },
    dispose() {
      cancel();
    },
  };
}
