// Quick actions shown on a clipboard card, plus the date popover they open.
//
// The card owns selection, editing, drag, and context-menu state; this module
// owns only the actions detected from the item text, their kind-deduped display
// list, and the date view.
import { invoke } from "@tauri-apps/api/core";
import {
  detectContentActions,
  type QuickAction,
  writeClipboardText,
} from "$lib/services/clipboard";
import { isTauriRuntime } from "$lib/services/runtime";
import { detectQuickActions, parseIsoDate, quickActionKind } from "$lib/utils/content/patterns";

export interface QuickActionsDeps {
  /** Read inside the reset effect so a recycled card clears its actions. */
  readonly id: string;
  /** Item text to scan: `textContent`, else title plus preview. */
  sourceText(): string;
}

export interface QuickActionsController {
  readonly actions: QuickAction[];
  readonly dateView: { isoDate: string; formattedDate: string; label: string } | null;
  load(): void;
  handleAction(event: MouseEvent, action: QuickAction): Promise<void>;
  dismissDate(): void;
}

export function createQuickActionsController(deps: QuickActionsDeps): QuickActionsController {
  let actions = $state<QuickAction[]>([]);
  let requestId = 0;
  let dateView = $state<{ isoDate: string; formattedDate: string; label: string } | null>(null);

  let loaded = $state(false);

  function load() {
    if (loaded) return;
    loaded = true;
    const text = deps.sourceText();
    const request = ++requestId;
    if (!isTauriRuntime()) {
      actions = detectQuickActions(text, true);
      return;
    }
    void detectContentActions(text)
      .then((detected) => {
        if (request === requestId) {
          const raw = detected ?? detectQuickActions(text, true);
          const seen = new Set<string>();
          actions = raw.filter((a) => {
            const key = `${a.actionType}:${a.payload}`;
            if (seen.has(key)) return false;
            seen.add(key);
            return true;
          });
        }
      })
      .catch(() => {
        if (request === requestId) {
          actions = detectQuickActions(text, true);
        }
      });
  }

  $effect(() => {
    const id = deps.id;
    loaded = false;
    actions = [];
    requestId = 0;
  });

  let visibleActions = $derived.by(() => {
    const seenKinds = new Set<string>();
    return actions.filter((action) => {
      const kind = quickActionKind(action);
      if (seenKinds.has(kind)) return false;
      seenKinds.add(kind);
      return true;
    });
  });

  function showDateDialog(action: QuickAction) {
    const date = parseIsoDate(action.payload);
    if (!date) {
      // Never log the payload: it is clipboard-derived content and does not
      // belong in diagnostics output.
      console.warn("Ignored invalid date action payload");
      return;
    }

    dateView = {
      isoDate: action.payload,
      formattedDate: new Intl.DateTimeFormat(undefined, {
        dateStyle: "full",
        timeZone: "UTC",
      }).format(date),
      label: action.label,
    };
  }

  async function handleAction(event: MouseEvent, action: QuickAction) {
    event.stopPropagation();
    switch (action.actionType) {
      case "open":
        try {
          await invoke("open_external_url", { url: action.payload });
        } catch {
          window.open(action.payload, "_blank");
        }
        return;
      case "copy":
        void writeClipboardText(action.payload).catch((err) =>
          console.error("Copy to clipboard failed:", err),
        );
        return;
      case "viewDate":
        await showDateDialog(action);
        return;
      default: {
        const unsupportedAction: never = action.actionType;
        console.warn("Ignored unsupported quick action", unsupportedAction);
      }
    }
  }

  return {
    get actions() {
      return visibleActions;
    },
    get dateView() {
      return dateView;
    },
    load,
    handleAction,
    dismissDate: () => (dateView = null),
  };
}
