import { describe, expect, it } from "vitest";

import { SETTINGS_SEARCH_ITEM_TEMPLATES } from "./settings-search";

const sectionFor = (id: string): string | undefined =>
  SETTINGS_SEARCH_ITEM_TEMPLATES.find((item) => item.id === id)?.section;

const fontSectionFor = (id: string): string | undefined =>
  SETTINGS_SEARCH_ITEM_TEMPLATES.find((item) => item.id === id)?.fontSection;

describe("settings search section targets", () => {
  // These ids are rendered by a panel that is lazily loaded for one section;
  // a stale `section` makes the settings search highlight nothing.
  it("points launch-at-startup at the general panel that renders it", () => {
    expect(sectionFor("general.launch-at-startup")).toBe("general_general");
  });

  it("points capture-size limits at the capture panel that renders them", () => {
    expect(sectionFor("general.max-text-capture-size")).toBe("capture");
    expect(sectionFor("storage.max-file-copy-size")).toBe("capture");
  });

  // The font panel renders two sub-tabs; without the target sub-tab a search
  // jump opens the interface sliders even for a card-font match.
  it("routes font search hits to the matching sub-tab", () => {
    expect(fontSectionFor("font.base")).toBe("interface");
    expect(fontSectionFor("font.secondary")).toBe("interface");
    expect(fontSectionFor("font.card-title")).toBe("card");
    expect(fontSectionFor("font.card-preview")).toBe("card");
  });
});
