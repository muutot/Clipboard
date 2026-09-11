/**
 * Pure keydown decision table for the search input, extracted from the main
 * route. Branch order and guards mirror the original handler exactly,
 * including the 400ms double-Backspace window and the bubble-through case
 * (closed suggestions + empty query returns plain `none` so the event keeps
 * bubbling to the global handler).
 */
export type SearchInputAction =
  | { type: "none" }
  | { type: "clear-query" }
  | { type: "accept-inline"; value: string }
  | { type: "move-index"; delta: 1 | -1 }
  | { type: "commit-query" }
  | { type: "choose-option"; value: string }
  | { type: "close-suggestions" };

export interface ResolvedSearchInputAction {
  action: SearchInputAction;
  /** Whether to preventDefault/stopPropagation for this key. */
  prevent: boolean;
  stop: boolean;
  /** Next `lastBackspaceAt` value the route must store. */
  backspaceAt: number;
}

export interface SearchInputContext {
  now: number;
  lastBackspaceAt: number;
  query: string;
  suggestionsOpen: boolean;
  optionCount: number;
  activeOption: string | null;
  inlineSuggestion: string | null;
  /** Caret collapsed at the end of the query (read from the input). */
  caretAtEnd: boolean;
}

const DOUBLE_BACKSPACE_MS = 400;

export function resolveSearchInputAction(
  event: KeyboardEvent,
  ctx: SearchInputContext,
): ResolvedSearchInputAction {
  const none = (backspaceAt = 0): ResolvedSearchInputAction => ({
    action: { type: "none" },
    prevent: false,
    stop: false,
    backspaceAt,
  });

  if (event.isComposing) return none(ctx.lastBackspaceAt);

  if (event.key === "Backspace") {
    if (ctx.now - ctx.lastBackspaceAt < DOUBLE_BACKSPACE_MS && ctx.query) {
      return {
        action: { type: "clear-query" },
        prevent: true,
        stop: false,
        backspaceAt: 0,
      };
    }
    return none(ctx.now);
  }

  if (
    (event.key === "Tab" || event.key === "ArrowRight") &&
    !event.shiftKey &&
    ctx.inlineSuggestion !== null &&
    ctx.caretAtEnd
  ) {
    return {
      action: { type: "accept-inline", value: ctx.inlineSuggestion },
      prevent: true,
      stop: true,
      backspaceAt: 0,
    };
  }

  if (
    (event.key === "ArrowDown" || event.key === "ArrowUp") &&
    ctx.suggestionsOpen &&
    ctx.optionCount > 0
  ) {
    return {
      action: { type: "move-index", delta: event.key === "ArrowDown" ? 1 : -1 },
      prevent: true,
      stop: true,
      backspaceAt: 0,
    };
  }

  if (event.key === "Enter") {
    if (ctx.activeOption) {
      return {
        action: { type: "choose-option", value: ctx.activeOption },
        prevent: true,
        stop: true,
        backspaceAt: 0,
      };
    }
    return { action: { type: "commit-query" }, prevent: true, stop: true, backspaceAt: 0 };
  }

  if (event.key === "Escape") {
    if (ctx.suggestionsOpen || ctx.query) {
      return {
        action: { type: "close-suggestions" },
        prevent: true,
        stop: true,
        backspaceAt: 0,
      };
    }
  }

  return none();
}
