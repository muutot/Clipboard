type MarkdownUrlKind = "link" | "image";

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function sanitizeUrl(raw: string, kind: MarkdownUrlKind): string | null {
  const value = raw.trim();
  if (!value || /[\u0000-\u001f\u007f]/.test(value)) return null;
  try {
    const base =
      typeof window === "undefined" ? "https://clipboard.invalid/" : window.location.href;
    const url = new URL(value, base);
    const allowed = kind === "image" ? ["http:", "https:"] : ["http:", "https:", "mailto:", "tel:"];
    return allowed.includes(url.protocol.toLowerCase()) ? escapeHtml(url.href) : null;
  } catch {
    return null;
  }
}

// Match source text once. Generated tags and attributes never re-enter the
// parser, so markdown in code or image alt text cannot turn into live markup.
function parseInline(text: string, allowLinks = true): string {
  const tokens =
    /`([^`\n]+)`|!\[([^\]]*)\]\(([^)]+)\)|\[([^\]]+)\]\(([^)]+)\)|\*\*([^*]+)\*\*|\*([^*]+)\*/g;
  const result: string[] = [];
  let cursor = 0;
  for (const match of text.matchAll(tokens)) {
    result.push(escapeHtml(text.slice(cursor, match.index)));
    const [, code, alt, imageUrl, label, linkUrl, bold, italic] = match;
    if (code !== undefined) {
      result.push(`<code class="md-inline-code">${escapeHtml(code)}</code>`);
    } else if (alt !== undefined) {
      const url = sanitizeUrl(imageUrl, "image");
      result.push(
        url
          ? `<img src="${url}" alt="${escapeHtml(alt)}" class="md-image" />`
          : `<span class="md-unsafe-image">${escapeHtml(alt)}</span>`,
      );
    } else if (label !== undefined) {
      const url = allowLinks ? sanitizeUrl(linkUrl, "link") : null;
      result.push(
        url
          ? `<a href="${url}" target="_blank" rel="noopener noreferrer" class="md-link">${parseInline(label, false)}</a>`
          : `<span class="md-unsafe-link">${escapeHtml(label)}</span>`,
      );
    } else if (bold !== undefined) {
      result.push(`<strong>${parseInline(bold, allowLinks)}</strong>`);
    } else {
      result.push(`<em>${parseInline(italic, allowLinks)}</em>`);
    }
    cursor = match.index + match[0].length;
  }
  result.push(escapeHtml(text.slice(cursor)));
  return result.join("");
}

/** Render the preview's supported Markdown subset with raw HTML always escaped. */
export function parseMarkdown(text: string): string {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const result: string[] = [];
  let paragraph: string[] = [];
  let list: "ul" | "ol" | null = null;
  const flushParagraph = () => {
    if (paragraph.length)
      result.push(
        `<p class="md-p">${paragraph.map((line) => parseInline(line)).join("<br />\n")}</p>`,
      );
    paragraph = [];
  };
  const closeList = () => {
    if (list) result.push(`</${list}>`);
    list = null;
  };

  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];
    const fence = line.match(/^ {0,3}(`{3,}|~{3,})([\w-]*)[ \t]*$/);
    if (fence) {
      flushParagraph();
      closeList();
      const code: string[] = [];
      const closeFence = new RegExp(`^ {0,3}${fence[1][0]}{${fence[1].length},}[ \\t]*$`);
      while (++index < lines.length && !closeFence.test(lines[index])) code.push(lines[index]);
      const body = code.join("\n") + (code.length && index < lines.length ? "\n" : "");
      result.push(
        `<pre class="md-code"><code class="language-${fence[2]}">${escapeHtml(body)}</code></pre>`,
      );
      continue;
    }
    if (!line.trim()) {
      flushParagraph();
      closeList();
      continue;
    }
    const item = line.match(/^[-*] (.+)$/) ?? line.match(/^\d+\. (.+)$/);
    if (item) {
      flushParagraph();
      const kind = /^\d/.test(line) ? "ol" : "ul";
      if (list !== kind) {
        closeList();
        list = kind;
        result.push(`<${kind} class="md-${kind}">`);
      }
      result.push(
        `<li class="${kind === "ol" ? "md-li-ordered" : "md-li"}">${parseInline(item[1])}</li>`,
      );
      continue;
    }
    closeList();
    const heading = line.match(/^(#{1,6}) (.+)$/);
    if (heading) {
      flushParagraph();
      const level = heading[1].length;
      result.push(`<h${level} class="md-h${level}">${parseInline(heading[2])}</h${level}>`);
    } else if (/^(---|\*\*\*|___)\s*$/.test(line)) {
      flushParagraph();
      result.push('<hr class="md-hr" />');
    } else if (/^> ?/.test(line)) {
      flushParagraph();
      const quote = [line.replace(/^> ?/, "")];
      while (index + 1 < lines.length && /^> ?/.test(lines[index + 1]))
        quote.push(lines[++index].replace(/^> ?/, ""));
      result.push(
        `<blockquote class="md-blockquote"><p>${quote.map((line) => parseInline(line)).join("<br />\n")}</p></blockquote>`,
      );
    } else {
      paragraph.push(line.trim());
    }
  }
  flushParagraph();
  closeList();
  return result.join("\n");
}
