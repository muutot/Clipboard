<script lang="ts">
  interface Props {
    content: string;
  }

  let { content }: Props = $props();

  const html = $derived(parseMarkdown(content));

  type MarkdownUrlKind = "link" | "image";

  /**
   * Only allow URLs that the preview can safely hand to the webview.  In
   * particular, clipboard content must never be able to create a javascript:
   * or data: URL inside the {@html} output below.
   */
  function sanitizeUrl(raw: string, kind: MarkdownUrlKind): string | null {
    const value = decodeHtmlEntities(raw).trim();
    if (!value || /[\u0000-\u001f\u007f]/.test(value)) return null;

    let parsed: URL;
    try {
      const base = typeof window === "undefined" ? "https://clipboard.invalid/" : window.location.href;
      parsed = new URL(value, base);
    } catch {
      return null;
    }

    const allowedSchemes = kind === "image"
      ? new Set(["http:", "https:"])
      : new Set(["http:", "https:", "mailto:", "tel:"]);
    if (!allowedSchemes.has(parsed.protocol.toLowerCase())) return null;

    return escapeHtml(parsed.href);
  }

  function parseMarkdown(text: string): string {
    let html = escapeHtml(text);

    // Code blocks (```)
    html = html.replace(/```(\w*)\n([\s\S]*?)```/g,
      (_m: string, lang: string, code: string) =>
        `<pre class="md-code"><code class="language-${lang}">${code.trim()}</code></pre>`);

    // Inline code
    html = html.replace(/`([^`]+)`/g, '<code class="md-inline-code">$1</code>');

    // Bold
    html = html.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');

    // Italic
    html = html.replace(/\*([^*]+)\*/g, '<em>$1</em>');

    // Images.  Unsafe URLs are kept as text instead of becoming a live
    // resource in the webview.
    html = html.replace(/!\[([^\]]*)\]\(([^)]+)\)/g,
      (_m: string, alt: string, rawUrl: string) => {
        const url = sanitizeUrl(rawUrl, "image");
        return url
          ? `<img src="${url}" alt="${alt}" class="md-image" />`
          : `<span class="md-unsafe-image">${alt}</span>`;
      });

    // Links.  Keep the label visible when a scheme is not explicitly allowed.
    html = html.replace(/\[([^\]]+)\]\(([^)]+)\)/g,
      (_m: string, label: string, rawUrl: string) => {
        const url = sanitizeUrl(rawUrl, "link");
        return url
          ? `<a href="${url}" target="_blank" rel="noopener noreferrer" class="md-link">${label}</a>`
          : `<span class="md-unsafe-link">${label}</span>`;
      });

    // Horizontal rules
    html = html.replace(/^(---|\*\*\*|___)\s*$/gm, '<hr class="md-hr" />');

    // Headings
    html = html.replace(/^###### (.+)$/gm, '<h6 class="md-h6">$1</h6>');
    html = html.replace(/^##### (.+)$/gm, '<h5 class="md-h5">$1</h5>');
    html = html.replace(/^#### (.+)$/gm, '<h4 class="md-h4">$1</h4>');
    html = html.replace(/^### (.+)$/gm, '<h3 class="md-h3">$1</h3>');
    html = html.replace(/^## (.+)$/gm, '<h2 class="md-h2">$1</h2>');
    html = html.replace(/^# (.+)$/gm, '<h1 class="md-h1">$1</h1>');

    // Blockquotes
    html = html.replace(/^> (.+)$/gm, '<blockquote class="md-blockquote"><p>$1</p></blockquote>');

    // Nested blockquotes - merge consecutive
    html = html.replace(/<\/blockquote>\n<blockquote[^>]*>/g, '\n');

    // Unordered lists - handle items
    html = html.replace(/^[\-\*] (.+)$/gm, '<li class="md-li">$1</li>');

    // Ordered lists
    html = html.replace(/^\d+\. (.+)$/gm, '<li class="md-li-ordered">$1</li>');

    // Wrap consecutive <li> in <ul> or <ol>
    html = html.replace(/((?:<li class="md-li">[^<]*<\/li>\n?)+)/g, '<ul class="md-ul">$1</ul>');
    html = html.replace(/((?:<li class="md-li-ordered">[^<]*<\/li>\n?)+)/g, '<ol class="md-ol">$1</ol>');

    // Paragraphs: wrap remaining non-empty non-tag lines
    const lines = html.split("\n");
    const result: string[] = [];
    let inParagraph = false;

    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed) {
        if (inParagraph) {
          result.push("</p>");
          inParagraph = false;
        }
        result.push("");
        continue;
      }

      if (/^<(\/?(?:h[1-6]|ul|ol|li|pre|blockquote|hr|p))/i.test(trimmed)) {
        if (inParagraph) {
          result.push("</p>");
          inParagraph = false;
        }
        result.push(trimmed);
        continue;
      }

      if (!inParagraph) {
        result.push('<p class="md-p">');
        inParagraph = true;
      } else {
        result.push('<br />');
      }
      result.push(trimmed);
    }

    if (inParagraph) {
      result.push("</p>");
    }

    return result.join("\n");
  }

  function escapeHtml(text: string): string {
    return text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#39;");
  }

  function decodeHtmlEntities(text: string): string {
    // The parser escapes text before applying markdown substitutions, so
    // decode only the entities we create when validating a captured URL.
    return text
      .replace(/&quot;/g, '"')
      .replace(/&#39;/g, "'")
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">")
      .replace(/&amp;/g, "&");
  }
</script>

{#if content}
  <div class="markdown-body">
    {@html html}
  </div>
{/if}

<style>
  .markdown-body {
    padding: 14px;
    border: 1px solid #2e2e2e;
    border-radius: 8px;
    color: #d4d4d4;
    background: #141414;
    font-size: 12.5px;
    line-height: 1.7;
    word-break: break-word;
  }

  :global(.markdown-body .md-h1) {
    margin: 0 0 10px;
    padding-bottom: 6px;
    border-bottom: 1px solid #2e2e2e;
    color: #f0f0f0;
    font-size: 18px;
    font-weight: 620;
  }

  :global(.markdown-body .md-h2) {
    margin: 12px 0 8px;
    padding-bottom: 4px;
    border-bottom: 1px solid #282828;
    color: #e8e8e8;
    font-size: 15px;
    font-weight: 590;
  }

  :global(.markdown-body .md-h3) {
    margin: 10px 0 6px;
    color: #e0e0e0;
    font-size: 13px;
    font-weight: 570;
  }

  :global(.markdown-body .md-h4),
  :global(.markdown-body .md-h5),
  :global(.markdown-body .md-h6) {
    margin: 8px 0 4px;
    color: #d0d0d0;
    font-size: 12px;
    font-weight: 550;
  }

  :global(.markdown-body .md-p) {
    margin: 2px 0 10px;
  }

  :global(.markdown-body .md-blockquote) {
    margin: 4px 0 10px;
    padding: 8px 12px;
    border-left: 3px solid #555;
    background: rgba(255, 255, 255, 0.03);
    border-radius: 0 4px 4px 0;
  }

  :global(.markdown-body .md-blockquote p) {
    margin: 0;
  }

  :global(.markdown-body .md-code) {
    margin: 8px 0;
    padding: 10px 14px;
    border: 1px solid #2e2e2e;
    border-radius: 6px;
    background: #0d0d0d;
    font: 11px/1.55 "Cascadia Code", Consolas, monospace;
    overflow-x: auto;
  }

  :global(.markdown-body .md-inline-code) {
    padding: 2px 5px;
    border: 1px solid #333;
    border-radius: 4px;
    color: #ce9178;
    background: #1a1a1a;
    font: 11px "Cascadia Code", Consolas, monospace;
  }

  :global(.markdown-body .md-ul),
  :global(.markdown-body .md-ol) {
    margin: 0 0 10px;
    padding-left: 22px;
  }

  :global(.markdown-body .md-li),
  :global(.markdown-body .md-li-ordered) {
    margin: 2px 0;
  }

  :global(.markdown-body .md-link) {
    color: #66bde1;
    text-decoration: none;
  }

  :global(.markdown-body .md-link:hover) {
    text-decoration: underline;
  }

  :global(.markdown-body .md-image) {
    max-width: 100%;
    border-radius: 6px;
    margin: 6px 0;
  }

  :global(.markdown-body .md-hr) {
    border: 0;
    border-top: 1px solid #333;
    margin: 12px 0;
  }

  :global(.markdown-body strong) {
    color: #eece99;
    font-weight: 600;
  }

  :global(.markdown-body em) {
    color: #bcbcbc;
  }
</style>
