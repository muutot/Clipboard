export interface SearchInteraction {
  generation: number;
  startedAt: number;
  /** Set once the interaction produced a sample, so a repeated search of the
   * same query cannot report the same measurement twice. */
  reported?: boolean;
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
  let frame = 0;
  const cancel = () => {
    if (frame) clock.cancel(frame);
    frame = 0;
  };
  return {
    /**
     * Starts a new interaction and returns it. The caller owns the returned
     * interaction until the matching search paints: starting it here (rather
     * than from a DOM input handler) keeps the measurement attached to the
     * query that actually produced the result, whatever changed the query.
     */
    input(): SearchInteraction {
      cancel();
      return { generation: ++generation, startedAt: clock.now() };
    },
    painted(interaction: SearchInteraction | null, current: () => boolean) {
      if (!interaction) return;
      if (interaction.reported) return;
      if (interaction.generation !== generation) return;
      if (!clock.visible()) return;
      cancel();
      frame = clock.frame(() => {
        frame = clock.frame(() => {
          frame = 0;
          if (interaction.reported) return;
          if (interaction.generation !== generation) return;
          if (!current()) return;
          if (!clock.visible()) return;
          const elapsed = clock.now() - interaction.startedAt;
          if (Number.isFinite(elapsed) && elapsed >= 0 && elapsed <= 60_000) {
            interaction.reported = true;
            report(Math.round(elapsed));
          }
        });
      });
    },
    dispose() {
      cancel();
      generation++;
    },
  };
}
