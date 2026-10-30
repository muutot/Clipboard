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
 * text nodes with `mark[data-find-match]` elements. Returns the match count.
 * Skips text inside existing marks so repeated application cannot stack.
 */
export function wrapMatches(container: HTMLElement, query: string): number {
  const needle = query.toLowerCase();
  if (!needle) return 0;

  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      if (!node.parentElement || node.parentElement.closest(MARK_SELECTOR)) {
        return NodeFilter.FILTER_REJECT;
      }
      return node.nodeValue && node.nodeValue.toLowerCase().includes(needle)
        ? NodeFilter.FILTER_ACCEPT
        : NodeFilter.FILTER_SKIP;
    },
  });

  // Collect first: replacing nodes while walking would revisit the marks
  // this pass just created.
  const targets: Text[] = [];
  while (walker.nextNode()) targets.push(walker.currentNode as Text);

  let count = 0;
  for (const node of targets) {
    const value = node.nodeValue ?? "";
    const lower = value.toLowerCase();
    const fragment = document.createDocumentFragment();
    let cursor = 0;
    let index = lower.indexOf(needle);
    while (index >= 0) {
      if (index > cursor) {
        fragment.appendChild(document.createTextNode(value.slice(cursor, index)));
      }
      const mark = document.createElement("mark");
      mark.dataset.findMatch = "";
      mark.textContent = value.slice(index, index + needle.length);
      fragment.appendChild(mark);
      count += 1;
      cursor = index + needle.length;
      index = lower.indexOf(needle, cursor);
    }
    if (cursor === 0) continue;
    if (cursor < value.length) {
      fragment.appendChild(document.createTextNode(value.slice(cursor)));
    }
    node.parentNode?.replaceChild(fragment, node);
  }
  return count;
}

/** Flags match `index` as the current one and scrolls it into view. */
export function setCurrentMatch(container: HTMLElement, index: number): void {
  const marks = container.querySelectorAll<HTMLElement>(MARK_SELECTOR);
  marks.forEach((mark, position) => {
    const current = position === index;
    mark.classList.toggle("find-current", current);
    if (current) mark.scrollIntoView({ block: "nearest" });
  });
}
