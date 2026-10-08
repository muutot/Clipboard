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
  tracker.input();
  const interaction = tracker.take();
  time = 320;
  tracker.painted(interaction, () => true);
  nextFrame();
  expect(report).not.toHaveBeenCalled();
  nextFrame();
  expect(report).toHaveBeenCalledWith(352);
  tracker.input();
  const stale = tracker.take();
  tracker.input();
  tracker.painted(stale, () => true);
  expect(frames.size).toBe(0);
  const fresh = tracker.take();
  tracker.painted(fresh, () => true);
  nextFrame();
  visible = false;
  nextFrame();
  expect(report).toHaveBeenCalledTimes(1);
  visible = true;
  tracker.input();
  tracker.painted(tracker.take(), () => true);
  tracker.dispose();
  expect(frames.size).toBe(0);
});
