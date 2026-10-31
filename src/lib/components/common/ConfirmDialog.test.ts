import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import ConfirmDialog from "./ConfirmDialog.svelte";
import { confirmDialog, settleConfirm } from "$lib/services/ui/confirm.svelte";
import { t } from "$lib/i18n";

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  // Release any request left pending by a failed assertion so the module
  // singleton does not leak into the next test.
  settleConfirm(false);
  for (const cleanup of cleanups.splice(0)) await cleanup();
});

beforeAll(() => {
  // jsdom cannot open real modal dialogs.
  HTMLDialogElement.prototype.showModal = function showModal() {
    this.setAttribute("open", "");
  };
});

function mountHost(): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(ConfirmDialog, { target });
  cleanups.push(async () => {
    await unmount(app);
    target.remove();
  });
  flushSync();
  return target;
}

const acceptButton = (target: HTMLElement) =>
  target.querySelector<HTMLButtonElement>(".confirm-accept")!;
const cancelButton = (target: HTMLElement) =>
  target.querySelector<HTMLButtonElement>(".confirm-cancel")!;

describe("ConfirmDialog", () => {
  it("resolves true when the confirm button is clicked", async () => {
    const target = mountHost();
    const pending = confirmDialog({ title: "Title", message: "Body" });
    flushSync();

    expect(target.querySelector(".confirm-title")?.textContent).toBe("Title");
    expect(target.querySelector(".confirm-message")?.textContent).toBe("Body");
    expect(acceptButton(target).textContent?.trim()).toBe(t("confirm.confirm"));
    acceptButton(target).click();
    flushSync();
    await expect(pending).resolves.toBe(true);
    expect(target.querySelector(".confirm-dialog")).toBeNull();
  });

  it("resolves false on cancel and removes the dialog", async () => {
    const target = mountHost();
    const pending = confirmDialog({ title: "Title", message: "Body" });
    flushSync();

    cancelButton(target).click();
    flushSync();
    await expect(pending).resolves.toBe(false);
    expect(target.querySelector(".confirm-dialog")).toBeNull();
  });

  it("supports destructive styling and custom labels", async () => {
    const target = mountHost();
    const pending = confirmDialog({
      title: "Title",
      message: "Body",
      confirmLabel: "Wipe",
      danger: true,
    });
    flushSync();

    expect(acceptButton(target).textContent?.trim()).toBe("Wipe");
    expect(acceptButton(target).classList.contains("danger")).toBe(true);
    acceptButton(target).click();
    flushSync();
    await expect(pending).resolves.toBe(true);
  });

  it("replaces a pending request and cancels the stale caller", async () => {
    const target = mountHost();
    const stale = confirmDialog({ title: "First", message: "Body" });
    flushSync();
    const fresh = confirmDialog({ title: "Second", message: "Body" });
    flushSync();

    expect(target.querySelector(".confirm-title")?.textContent).toBe("Second");
    await expect(stale).resolves.toBe(false);
    acceptButton(target).click();
    flushSync();
    await expect(fresh).resolves.toBe(true);
  });
});
