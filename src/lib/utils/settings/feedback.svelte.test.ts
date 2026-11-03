import { afterEach, expect, it, vi } from "vitest";
import { createFeedback } from "./feedback.svelte";

afterEach(() => {
  vi.useRealTimers();
});

it("ignores feedback and schedules no timer after disposal", () => {
  vi.useFakeTimers();
  const feedback = createFeedback(3000);

  feedback.dispose();
  feedback.show("late result");

  expect(feedback.message).toBe("");
  expect(vi.getTimerCount()).toBe(0);
});
