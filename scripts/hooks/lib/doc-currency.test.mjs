import { describe, expect, it } from "vitest";
import { candidateReferences, evaluateDocGate, isDocumentedSource } from "./doc-currency.mjs";

describe("documentation currency gate", () => {
  it("ignores paths the references do not describe", () => {
    const ignored = [
      "package.json",
      "docs/PITFALLS.md",
      "src/lib/utils/search/demo-search.test.ts",
      "src-tauri/Cargo.lock",
      "skills/clipboard-dev/references/services.md",
    ];
    for (const path of ignored) expect(isDocumentedSource(path), path).toBe(false);
  });

  it("maps settings panels, routes and search utilities to their references", () => {
    const settings = candidateReferences(["src/lib/components/settings/SyncPanel.svelte"]);
    expect(settings).toContain("settings-panels.md");
    expect(settings).toContain("css-theming.md");
    expect(candidateReferences(["src/routes/float/+page.svelte"])).toContain("components.md");
    expect(candidateReferences(["src/lib/utils/search/demo-search.ts"])).toContain(
      "search-cache-strategy.md",
    );
    expect(candidateReferences(["src-tauri/src/search/mod.rs"])).toContain(
      "backend-architecture.md",
    );
  });

  it("blocks a new source module until a reference is staged with it", () => {
    const gate = evaluateDocGate({
      stagedPaths: ["src/lib/services/backup.ts"],
      addedOrRemovedPaths: ["src/lib/services/backup.ts"],
    });
    expect(gate.status).toBe("missing");
    expect(gate.refs).toContain("services.md");
  });

  it("accepts the gate once the matching reference is staged", () => {
    const gate = evaluateDocGate({
      stagedPaths: ["src/lib/services/backup.ts", "skills/clipboard-dev/references/services.md"],
      addedOrRemovedPaths: ["src/lib/services/backup.ts"],
    });
    expect(gate.status).toBe("satisfied");
  });

  it("reminds instead of blocking for a plain source edit", () => {
    const gate = evaluateDocGate({
      stagedPaths: ["src/routes/float/+page.svelte"],
      addedOrRemovedPaths: [],
    });
    expect(gate.status).toBe("reminder");
    expect(gate.refs).toEqual(["components.md"]);
  });

  it("reports not-applicable when no documented source is staged", () => {
    expect(evaluateDocGate({ stagedPaths: ["package.json"], addedOrRemovedPaths: [] }).status).toBe(
      "not-applicable",
    );
  });
});
