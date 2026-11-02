import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { writeClipboardHtml, writeClipboardImage, writeClipboardText } from "./clipboard";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), writeText: vi.fn(), write: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: bridge.invoke,
  convertFileSrc: (x: string) => x,
}));
vi.mock("$lib/services/settings", () => ({
  generalSettings: {
    subscribe(run: (value: unknown) => void) {
      run({});
      return () => {};
    },
  },
}));

beforeEach(() => {
  bridge.invoke.mockReset().mockResolvedValue(["registration"]);
  bridge.writeText.mockReset().mockResolvedValue(undefined);
  bridge.write.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  vi.stubGlobal("navigator", { clipboard: { writeText: bridge.writeText, write: bridge.write } });
  vi.stubGlobal(
    "ClipboardItem",
    class {
      constructor(public readonly data: unknown) {}
    },
  );
  vi.spyOn(console, "warn").mockImplementation(() => {});
});
afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const writers = [
  {
    kind: "text",
    run: () => writeClipboardText("same"),
    native: () => bridge.writeText,
    mark: "mark_self_triggered",
    unmark: "unmark_self_triggered",
  },
  {
    kind: "html",
    run: () => writeClipboardHtml("<b>same</b>", "same"),
    native: () => bridge.write,
    mark: "mark_self_triggered",
    unmark: "unmark_self_triggered",
  },
  {
    kind: "image",
    run: () =>
      writeClipboardImage(new Blob(["same"], { type: "image/png" }), "image.png", "same-hash"),
    native: () => bridge.write,
    mark: "mark_self_triggered_image",
    unmark: "unmark_self_triggered_image",
  },
];

for (const writer of writers) {
  describe(`${writer.kind} self-trigger ownership`, () => {
    it("clears only the delayed failed attempt after another write succeeds", async () => {
      bridge.invoke
        .mockResolvedValueOnce(["failed-attempt"])
        .mockResolvedValueOnce(["successful-attempt"]);
      const failure = new Error("delayed clipboard refusal");
      let rejectWrite!: (error: Error) => void;
      writer.native().mockImplementationOnce(
        () =>
          new Promise<void>((_, reject) => {
            rejectWrite = reject;
          }),
      );
      const delayed = writer.run();
      const rejected = expect(delayed).rejects.toBe(failure);
      await vi.waitFor(() => expect(writer.native()).toHaveBeenCalledTimes(1));
      await writer.run();
      rejectWrite(failure);
      await rejected;
      expect(bridge.invoke).toHaveBeenLastCalledWith(writer.unmark, {
        registrations: ["failed-attempt"],
      });
      expect(
        bridge.invoke.mock.calls.filter(([command]) => command === writer.unmark),
      ).toHaveLength(1);
    });

    it("retains the registration after a successful write", async () => {
      await writer.run();
      expect(bridge.invoke.mock.calls.map(([command]) => command)).toEqual([writer.mark]);
      expect(writer.native()).toHaveBeenCalledTimes(1);
    });

    it("does not clear other markers when registration itself failed", async () => {
      bridge.invoke.mockRejectedValueOnce(new Error("registration unavailable"));
      const failure = new Error("clipboard refusal");
      writer.native().mockRejectedValueOnce(failure);
      await expect(writer.run()).rejects.toBe(failure);
      expect(bridge.invoke.mock.calls.map(([command]) => command)).toEqual([writer.mark]);
    });

    it("keeps browser writes independent of Tauri registration", async () => {
      Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
      await writer.run();
      expect(bridge.invoke).not.toHaveBeenCalled();
      expect(writer.native()).toHaveBeenCalledTimes(1);
    });
  });
}

it("preserves the original write error when receipt cleanup fails", async () => {
  const failure = new Error("clipboard refusal");
  bridge.writeText.mockRejectedValueOnce(failure);
  bridge.invoke
    .mockResolvedValueOnce(["registration"])
    .mockRejectedValueOnce(new Error("cleanup unavailable"));
  await expect(writeClipboardText("same")).rejects.toBe(failure);
  expect(bridge.invoke).toHaveBeenLastCalledWith("unmark_self_triggered", {
    registrations: ["registration"],
  });
});
