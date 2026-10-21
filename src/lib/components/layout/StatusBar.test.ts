import { mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it } from "vitest";
import StatusBar from "./StatusBar.svelte";

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  target.remove();
});

function render(resultSummary: string) {
  component = mount(StatusBar, {
    target,
    props: {
      resultSummary,
      runtimeLabel: "windows / x86_64 · 核心已连接",
      statusMessage: "使用 ↑ ↓ 选择，Enter 快速粘贴",
    },
  });
  const footer = target.querySelector("footer")!;
  return {
    footer,
    count: footer.querySelector(".result-count")!,
    order: Array.from(footer.children).map((child) => child.classList[0]),
  };
}

it("keeps the record count at the trailing edge so a loading count cannot move the bar", () => {
  const { footer, count, order } = render("记录数: 89");

  expect(count.textContent).toBe("记录数: 89");
  expect(footer.lastElementChild).toBe(count);
  expect(order).toEqual(["status-left", "status-msg", "result-count"]);
});

it("renders no shortcut hint next to the count", () => {
  render("记录数: 89");

  expect(target.querySelector("kbd")).toBeNull();
  expect(target.querySelector(".shortcut-hints")).toBeNull();
});
