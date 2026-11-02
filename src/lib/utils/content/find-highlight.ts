/**
 * Find-in-detail highlight helpers. The detail panel renders its preview
 * through three different pipelines (plain `<pre>` text, CodePreview's
 * tokenizer spans, MarkdownPreview's `{@html}` output), so matches are
 * applied by walking the rendered DOM instead of restructuring each
 * renderer. Callers must `unwrapMatches` before re-applying `wrapMatches` —
 * unwrapping first is the only guard against stacked duplicate marks.
 */

const MARK_SELECTOR = "mark[data-find-match]";

/** Removes every find mark below `container`, restoring the original text. */
export function unwrapMatches(container: HTMLElement): void {
  const marks = Array.from(container.querySelectorAll<HTMLElement>(MARK_SELECTOR));
  for (const mark of marks) {
    const parent = mark.parentNode;
    if (!parent) continue;
    parent.replaceChild(document.createTextNode(mark.textContent ?? ""), mark);
    parent.normalize();
  }
}

/**
 * Wraps every case-insensitive literal occurrence of `query` in `container`'s
 * inline text runs with `mark[data-find-match]` elements. Returns the logical
 * match count; fragments across inline markup share a match ID for navigation.
 * Skips existing marks and UI-only subtrees annotated with data-find-ignore.
 */
export function wrapMatches(container: HTMLElement, query: string): number {
  if (!query) return 0;
  // Lowercasing can expand characters (İ -> i + combining dot), so indices
  // from a lowercased copy cannot be used to slice the original DOM text.
  const pattern = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "giu");

  interface TextRun {
    block: Element;
    text: string;
    parts: { node: Text; start: number; end: number }[];
  }
  const runs: TextRun[] = [];
  let run: TextRun | null = null;

  const walker = document.createTreeWalker(
    container,
    NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT,
    {
      acceptNode(node) {
        if (node instanceof Element) {
          if (node.matches(`${MARK_SELECTOR}, [data-find-ignore]`)) {
            run = null;
            return NodeFilter.FILTER_REJECT;
          }
          if (node.matches("br, img, hr")) run = null;
          return NodeFilter.FILTER_SKIP;
        }
        return node.nodeValue ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP;
      },
    },
  );

  // Collect first: replacing nodes while walking would revisit the marks
  // this pass just created.
  while (walker.nextNode()) {
    const node = walker.currentNode as Text;
    const block =
      node.parentElement?.closest(
        "[data-find-block], div, p, li, blockquote, pre, h1, h2, h3, h4, h5, h6",
      ) ?? container;
    if (!run || run.block !== block) {
      run = { block, text: "", parts: [] };
      runs.push(run);
    }
    const start = run.text.length;
    run.text += node.data;
    run.parts.push({ node, start, end: run.text.length });
  }

  let count = 0;
  for (const textRun of runs) {
    const matches = [...textRun.text.matchAll(pattern)];
    const firstId = count;
    count += matches.length;
    let firstMatch = 0;
    for (const part of textRun.parts) {
      while (
        firstMatch < matches.length &&
        matches[firstMatch].index + matches[firstMatch][0].length <= part.start
      ) {
        firstMatch++;
      }
      const value = part.node.data;
      const fragment = document.createDocumentFragment();
      let cursor = 0;
      for (
        let index = firstMatch;
        index < matches.length && matches[index].index < part.end;
        index++
      ) {
        const match = matches[index];
        const start = Math.max(match.index, part.start) - part.start;
        const end = Math.min(match.index + match[0].length, part.end) - part.start;
        if (start > cursor)
          fragment.appendChild(document.createTextNode(value.slice(cursor, start)));
        const mark = document.createElement("mark");
        mark.dataset.findMatch = String(firstId + index);
        mark.textContent = value.slice(start, end);
        fragment.appendChild(mark);
        cursor = end;
      }
      if (cursor === 0) continue;
      if (cursor < value.length) fragment.appendChild(document.createTextNode(value.slice(cursor)));
      part.node.parentNode?.replaceChild(fragment, part.node);
    }
  }
  return count;
}

/** Flags match `index` as the current one and scrolls it into view. */
export function setCurrentMatch(container: HTMLElement, index: number): void {
  const marks = container.querySelectorAll<HTMLElement>(MARK_SELECTOR);
  let scrolled = false;
  marks.forEach((mark) => {
    const current = mark.dataset.findMatch === String(index);
    mark.classList.toggle("find-current", current);
    if (current && !scrolled) {
      mark.scrollIntoView({ block: "nearest" });
      scrolled = true;
    }
  });
}
