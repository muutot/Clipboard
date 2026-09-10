// Shared auto-clearing feedback state for settings panels. Replaces the
// per-panel `feedback`/`feedbackSuccess`/`feedbackTimer` trio (plus its
// onMount/onDestroy cleanup) previously copied across Keyboard, Sensitive,
// and General panels. TagManagement keeps its own `{message, kind} | null`
// shape and is intentionally out of scope.

export interface PanelFeedback {
  readonly message: string;
  readonly success: boolean;
  show(message: string, success?: boolean): void;
  clear(): void;
  dispose(): void;
}

export function createFeedback(timeoutMs = 3000): PanelFeedback {
  let message = $state("");
  let success = $state(true);
  let timer: ReturnType<typeof setTimeout> | undefined;

  function clearTimer() {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
  }

  return {
    get message() {
      return message;
    },
    get success() {
      return success;
    },
    show(next: string, ok = true) {
      message = next;
      success = ok;
      clearTimer();
      timer = setTimeout(() => {
        timer = undefined;
        message = "";
      }, timeoutMs);
    },
    clear() {
      clearTimer();
      message = "";
    },
    dispose() {
      clearTimer();
    },
  };
}
