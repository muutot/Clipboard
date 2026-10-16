import { describe, expect, it } from "vitest";
import { selectionOrder } from "./selection-order";

describe("selection order", () => {
  it("numbers items by selection order, not list order", () => {
    const order = selectionOrder(["third", "first", "second"]);
    expect(order.get("third")).toBe(1);
    expect(order.get("first")).toBe(2);
    expect(order.get("second")).toBe(3);
  });

  it("keeps the first position when an id repeats", () => {
    const order = selectionOrder(["a", "b", "a"]);
    expect(order.get("a")).toBe(1);
    expect(order.get("b")).toBe(2);
    expect(order.size).toBe(2);
  });

  it("returns an empty map for an empty selection", () => {
    expect(selectionOrder(new Set()).size).toBe(0);
  });
});
