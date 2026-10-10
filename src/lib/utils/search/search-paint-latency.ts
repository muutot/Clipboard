export interface SearchInteraction {
  generation: number;
  startedAt: number;
}
interface Clock {
  now(): number;
  frame(callback: FrameRequestCallback): number;
  cancel(id: number): void;
  visible(): boolean;
}
const browserClock: Clock = {
  now: () => performance.now(),
  frame: (callback) => requestAnimationFrame(callback),
  cancel: (id) => cancelAnimationFrame(id),
  visible: () => document.visibilityState === "visible",
};
/** Durations only: never retains query text or clipboard contents. Double rAF approximates a rendered frame. */
export function createSearchPaintTracker(
  report: (ms: number) => void,
  clock: Clock = browserClock,
) {
  let generation = 0;
  let pending: SearchInteraction | null = null;
  let frame = 0;
  const cancel = () => {
    if (frame) clock.cancel(frame);
    frame = 0;
  };
  return {
    input() {
      cancel();
      pending = { generation: ++generation, startedAt: clock.now() };
    },
    take() {
      const interaction = pending;
      pending = null;
      return interaction;
    },
    painted(interaction: SearchInteraction | null, current: () => boolean) {
      if (!interaction || interaction.generation !== generation || !clock.visible()) return;
      cancel();
      frame = clock.frame(() => {
        frame = clock.frame(() => {
          frame = 0;
          if (interaction.generation !== generation || !current() || !clock.visible()) return;
          const elapsed = clock.now() - interaction.startedAt;
          if (Number.isFinite(elapsed) && elapsed >= 0 && elapsed <= 60_000)
            report(Math.round(elapsed));
        });
      });
    },
    dispose() {
      cancel();
      generation++;
      pending = null;
    },
  };
}
