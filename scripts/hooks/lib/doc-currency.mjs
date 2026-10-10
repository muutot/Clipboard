// Documentation currency gate helpers (skills/clipboard-dev/SKILL.md).
// Maps staged source paths to the reference files that probably need an update.
// The mapping is a heuristic: it names candidates, it does not decide the gate.

const REFERENCE_PREFIX = "skills/clipboard-dev/";

// Every matching rule contributes, so a narrow rule can add to a broad one.
const ROUTES = [
  { pattern: /^src\/lib\/components\/settings\//, refs: ["settings-panels.md", "css-theming.md"] },
  { pattern: /^src\/lib\/components\//, refs: ["components.md"] },
  { pattern: /^src\/routes\//, refs: ["components.md"] },
  { pattern: /\.css$/i, refs: ["css-theming.md"] },
  { pattern: /^src\/lib\/services\//, refs: ["services.md"] },
  { pattern: /^src\/lib\/controllers\//, refs: ["components.md", "services.md"] },
  { pattern: /^src\/lib\/i18n\//, refs: ["data-contracts.md"] },
  { pattern: /^src\/lib\/types\//, refs: ["data-contracts.md"] },
  { pattern: /^src\/lib\/utils\/search\//, refs: ["search-cache-strategy.md"] },
  { pattern: /^src\/lib\/utils\/keyboard\//, refs: ["hotkey-architecture.md"] },
  { pattern: /^src\/lib\/utils\/settings\//, refs: ["settings-reference.md"] },
  { pattern: /^src\/lib\//, refs: ["project-structure.md"] },
  { pattern: /^src-tauri\//, refs: ["backend-architecture.md"] },
];

const IGNORED_EXTENSIONS = /\.(md|json|lock|toml|txt|snap|svg|png|ico)$/i;
const TEST_FILE = /\.(test|spec)\.[cm]?[jt]sx?$/i;
const IGNORED_PREFIXES = ["skills/", "docs/", ".github/", "scripts/", ".githooks/"];

export function isReferencePath(path) {
  return path.startsWith(REFERENCE_PREFIX);
}

/** True when the path is source this repository documents in a reference file. */
export function isDocumentedSource(path) {
  if (isReferencePath(path)) return false;
  if (IGNORED_PREFIXES.some((prefix) => path.startsWith(prefix))) return false;
  if (IGNORED_EXTENSIONS.test(path)) return false;
  if (TEST_FILE.test(path)) return false;
  return /^src\/|^src-tauri\//.test(path);
}

export function candidateReferences(paths) {
  const refs = [];
  for (const path of paths) {
    for (const route of ROUTES) {
      if (!route.pattern.test(path)) continue;
      for (const ref of route.refs) if (!refs.includes(ref)) refs.push(ref);
    }
  }
  return refs;
}

/**
 * Classifies the documentation currency gate for one staged change set.
 * - not-applicable: no documented source touched
 * - satisfied: a skills/clipboard-dev path is staged as well
 * - missing: a source module was added or removed without a reference change
 * - reminder: source edited only; the gate is a judgement call
 */
export function evaluateDocGate({ stagedPaths, addedOrRemovedPaths }) {
  const source = stagedPaths.filter(isDocumentedSource);
  if (source.length === 0) return { status: "not-applicable", refs: [] };
  const refs = candidateReferences(source);
  const referencesTouched = stagedPaths.filter(isReferencePath);
  if (referencesTouched.length > 0) return { status: "satisfied", refs, referencesTouched };
  const structural = addedOrRemovedPaths.filter(isDocumentedSource);
  if (structural.length > 0) return { status: "missing", refs, structural };
  return { status: "reminder", refs, source };
}
