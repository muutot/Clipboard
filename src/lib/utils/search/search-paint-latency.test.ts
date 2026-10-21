import { expect, it, vi } from "vitest";
import { createSearchPaintTracker } from "./search-paint-latency";
it("includes debounce and rendering, drops stale/hidden work and cancels on disposal", () => {
  let time = 0,
    visible = true,
    sequence = 0;
  const frames = new Map<number, FrameRequestCallback>(),
    report = vi.fn();
  const clock = {
    now: () => time,
    visible: () => visible,
    frame: (callback: FrameRequestCallback) => {
      frames.set(++sequence, callback);
      return sequence;
    },
    cancel: (id: number) => {
      frames.delete(id);
    },
  };
  const nextFrame = () => {
    const callbacks = [...frames.values()];
    frames.clear();
    time += 16;
    callbacks.forEach((callback) => callback(time));
  };
  const tracker = createSearchPaintTracker(report, clock);
  const interaction = tracker.input();
  time = 320;
  tracker.painted(interaction, () => true);
  nextFrame();
  expect(report).not.toHaveBeenCalled();
  nextFrame();
  expect(report).toHaveBeenCalledWith(352);
  // A newer interaction makes the previous one stale: it is dropped without
  // scheduling a frame.
  const stale = tracker.input();
  tracker.input();
  tracker.painted(stale, () => true);
  expect(frames.size).toBe(0);
  const fresh = tracker.input();
  tracker.painted(fresh, () => true);
  nextFrame();
  visible = false;
  nextFrame();
  expect(report).toHaveBeenCalledTimes(1);
  visible = true;
  // A superseded request never reports.
  const superseded = tracker.input();
  tracker.painted(superseded, () => false);
  nextFrame();
  nextFrame();
  expect(report).toHaveBeenCalledTimes(1);
  const pending = tracker.input();
  tracker.painted(pending, () => true);
  tracker.dispose();
  expect(frames.size).toBe(0);
});
