<script lang="ts">
  interface Props {
    content: string;
    language?: string | null;
  }

  import { FILE_PREVIEW_LIMIT } from "$lib/utils/content/file-preview";

  let { content, language = null }: Props = $props();

  // The tokenizer and the per-line spans have no other bound: without a cap,
  // opening the detail panel on a multi-hundred-KB code item tokenizes and
  // renders every byte at once. Cap at the same limit as file previews and
  // reuse the shared truncation note.
  const preview = $derived.by(() => {
    if (content.length <= FILE_PREVIEW_LIMIT) {
      return { text: content, truncated: false };
    }
    // Cut back to the last line boundary inside the limit so the rendered
    // block does not end on a half-token line.
    const cut = content.lastIndexOf("\n", FILE_PREVIEW_LIMIT);
    return {
      text: cut > 0 ? content.slice(0, cut) : content.slice(0, FILE_PREVIEW_LIMIT),
      truncated: true,
    };
  });

  const detectedLang = $derived(language ?? detectLanguage(preview.text));

  const tokenizedLines = $derived(tokenize(preview.text, detectedLang));

  function detectLanguage(text: string): string {
    const patterns: { name: string; re: RegExp; weight: number }[] = [
      {
        name: "TypeScript",
        re: /^(import|export)\s|:\s*(string|number|boolean|void|any)\b|interface\s+\w+\s*\{|type\s+\w+\s*=/m,
        weight: 3,
      },
      { name: "Rust", re: /^(use\s+|fn\s+|let\s+mut\s|struct\s|impl\s|pub\s|mod\s)/m, weight: 3 },
      {
        name: "Python",
        re: /^(def\s+|class\s+\w+.*:|import\s+\w+|from\s+\w+\s+import|print\(|elif\s)/m,
        weight: 3,
      },
      { name: "JSON", re: /^\s*[{\[]\s*$|"[\w-]+"\s*:\s*|^\s*[}\]]\s*,?\s*$/m, weight: 2 },
      {
        name: "HTML",
        re: /^<!DOCTYPE|<html\b|<head\b|<body\b|<div\b|<span\b|<script\b|<style\b/m,
        weight: 3,
      },
      {
        name: "SQL",
        re: /^(SELECT|INSERT|UPDATE|DELETE|CREATE|ALTER|DROP)\s|^\s*(FROM|WHERE|JOIN|ORDER BY|GROUP BY)\s/im,
        weight: 3,
      },
      { name: "Shell", re: /^#!\//m, weight: 4 },
      {
        name: "Shell",
        re: /^(echo |cd |ls |grep |mkdir |sudo |apt |npm |yarn |git |docker |export\s)/m,
        weight: 2,
      },
      {
        name: "CSS",
        re: /^[.#@][\w-]+\s*\{|^\s*(color|margin|padding|display|font-size|background|border):\s/m,
        weight: 2,
      },
      {
        name: "JavaScript",
        re: /^(function\s+\w+|var\s+|const\s+|let\s+|console\.|document\.|window\.|require\()/m,
        weight: 2,
      },
    ];

    let best = { name: "", weight: 0 };
    for (const { name, re, weight } of patterns) {
      if (re.test(text) && weight > best.weight) {
        best = { name, weight };
      }
    }
    return best.name || "";
  }

  const keywords: Record<string, Set<string>> = {
    // Plain text and markdown get no keyword highlighting — the tokenizer
    // falls back to JavaScript keywords for any language missing from this map.
    markdown: new Set(),
    text: new Set(),
    JavaScript: new Set([
      "function",
      "var",
      "const",
      "let",
      "return",
      "if",
      "else",
      "for",
      "while",
      "class",
      "new",
      "this",
      "null",
      "undefined",
      "true",
      "false",
      "import",
      "export",
      "default",
      "from",
      "await",
      "async",
      "try",
      "catch",
      "throw",
      "typeof",
      "instanceof",
      "of",
      "in",
    ]),
    TypeScript: new Set([
      "function",
      "var",
      "const",
      "let",
      "return",
      "if",
      "else",
      "for",
      "while",
      "class",
      "new",
      "this",
      "null",
      "undefined",
      "true",
      "false",
      "import",
      "export",
      "default",
      "from",
      "await",
      "async",
      "try",
      "catch",
      "throw",
      "typeof",
      "instanceof",
      "of",
      "in",
      "interface",
      "type",
      "enum",
      "implements",
      "extends",
      "as",
      "is",
      "readonly",
      "private",
      "public",
      "protected",
    ]),
    Rust: new Set([
      "fn",
      "let",
      "mut",
      "struct",
      "impl",
      "pub",
      "mod",
      "use",
      "match",
      "if",
      "else",
      "for",
      "while",
      "loop",
      "return",
      "self",
      "true",
      "false",
      "async",
      "await",
      "enum",
      "trait",
      "where",
      "ref",
      "move",
      "unsafe",
      "extern",
      "crate",
      "super",
      "in",
      "const",
      "static",
      "type",
      "as",
    ]),
    Python: new Set([
      "def",
      "class",
      "import",
      "from",
      "return",
      "if",
      "elif",
      "else",
      "for",
      "while",
      "try",
      "except",
      "finally",
      "with",
      "as",
      "pass",
      "break",
      "continue",
      "lambda",
      "yield",
      "raise",
      "True",
      "False",
      "None",
      "and",
      "or",
      "not",
      "in",
      "is",
      "print",
      "self",
      "global",
      "nonlocal",
      "del",
      "assert",
    ]),
    SQL: new Set([
      "SELECT",
      "FROM",
      "WHERE",
      "INSERT",
      "UPDATE",
      "DELETE",
      "CREATE",
      "ALTER",
      "DROP",
      "TABLE",
      "INDEX",
      "JOIN",
      "INNER",
      "LEFT",
      "RIGHT",
      "OUTER",
      "ON",
      "AND",
      "OR",
      "NOT",
      "NULL",
      "IS",
      "IN",
      "BETWEEN",
      "LIKE",
      "ORDER",
      "BY",
      "GROUP",
      "HAVING",
      "LIMIT",
      "OFFSET",
      "AS",
      "SET",
      "VALUES",
      "INTO",
      "DISTINCT",
      "COUNT",
      "SUM",
      "AVG",
      "MAX",
      "MIN",
      "EXISTS",
      "CASE",
      "WHEN",
      "THEN",
      "ELSE",
      "END",
      "UNION",
      "ALL",
      "PRIMARY",
      "KEY",
      "FOREIGN",
      "REFERENCES",
      "CONSTRAINT",
      "DEFAULT",
      "CHECK",
      "UNIQUE",
      "CASCADE",
      "BEGIN",
      "COMMIT",
      "ROLLBACK",
    ]),
  };

  function tokenize(text: string, lang: string): { text: string; classes: string }[][] {
    const lines = text.split("\n");
    const kw = lang in keywords ? keywords[lang] : keywords.JavaScript;

    return lines.map((line) => {
      const tokens: { text: string; classes: string }[] = [];

      // Comments
      if (lang === "Python" || lang === "Shell") {
        const ci = line.indexOf("#");
        if (ci >= 0) {
          if (ci > 0) tokens.push({ text: line.slice(0, ci), classes: "" });
          tokens.push({ text: line.slice(ci), classes: "token-comment" });
          return tokens;
        }
      }

      if (["JavaScript", "TypeScript", "Rust", "CSS"].includes(lang)) {
        const ci = line.indexOf("//");
        if (ci >= 0) {
          if (ci > 0) tokens.push({ text: line.slice(0, ci), classes: "" });
          tokens.push({ text: line.slice(ci), classes: "token-comment" });
          return tokens;
        }
      }

      if (lang === "SQL") {
        const ci = line.indexOf("--");
        if (ci >= 0) {
          if (ci > 0) tokens.push({ text: line.slice(0, ci), classes: "" });
          tokens.push({ text: line.slice(ci), classes: "token-comment" });
          return tokens;
        }
      }

      // String matching
      const re =
        /("[^"\\]*(?:\\.[^"\\]*)*"|'[^'\\]*(?:\\.[^'\\]*)*'|`[^`\\]*(?:\\.[^`\\]*)*`|[\w]+|[^\w\s]|\s+)/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(line)) !== null) {
        const part = m[0];
        if (/^["'`]/.test(part) && /["'`]$/.test(part) && part.length > 1) {
          tokens.push({ text: part, classes: "token-string" });
        } else if (kw.has(part)) {
          tokens.push({ text: part, classes: "token-keyword" });
        } else if (/^\d+(\.\d+)?$/.test(part)) {
          tokens.push({ text: part, classes: "token-number" });
        } else if (lang === "JSON" && /^"[\w-]+"$/.test(part)) {
          tokens.push({ text: part, classes: "token-key" });
        } else {
          tokens.push({ text: part, classes: "" });
        }
      }

      return tokens;
    });
  }
</script>

{#if content}
  <div class="code-block">
    {#if detectedLang}
      <div class="code-lang-label">{detectedLang}</div>
    {/if}
    <pre class="code-content"><code
        >{#each tokenizedLines as line, i}<span class="code-line"
            ><span class="line-number"
              >{String(i + 1).padStart(String(tokenizedLines.length).length, " ")}</span
            >{#each line as token}{#if token.classes}<span class={token.classes}>{token.text}</span
                >{:else}{token.text}{/if}{/each}</span
          >
        {/each}</code
      ></pre>
  </div>
{/if}

<style>
  .code-block {
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    overflow: hidden;
    background: var(--input-bg);
  }

  .code-lang-label {
    display: inline-block;
    padding: 3px 10px;
    margin: 8px 10px 0;
    border: 1px solid var(--border-color);
    border-radius: 5px;
    color: var(--text-muted);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .code-truncated {
    display: block;
    max-width: 100%;
    padding: 4px 10px 8px;
    color: var(--text-muted);
    font-size: 10px;
    text-align: center;
  }

  .code-content {
    margin: 0;
    padding: 8px 0;
    overflow-x: auto;
    font:
      13px/1.6 "Cascadia Code",
      Consolas,
      "SFMono-Regular",
      monospace;
    color: var(--text-primary);
  }

  /* Flex column: pure-whitespace text nodes (prettier formatting inside the
     pre) are ignored as flex items, so no phantom compressed line boxes —
     the old line-height 0.5 hack painted overlapping selection rects. */
  .code-content code {
    display: flex;
    flex-direction: column;
    width: max-content;
    min-width: 100%;
  }

  .code-line {
    display: block;
    padding-right: 16px;
    white-space: pre;
  }

  .line-number {
    display: inline-block;
    width: 36px;
    padding-right: 12px;
    text-align: right;
    color: var(--text-faint);
    user-select: none;
    flex-shrink: 0;
  }

  /* Syntax token hues are a fixed niche palette (content-defined rendering);
     mixing toward --text-primary shifts their luminance with the theme so
     they stay legible on both dark and light code surfaces. */
  :global(.token-keyword) {
    color: color-mix(in srgb, #c586c0 75%, var(--text-primary));
  }

  :global(.token-string) {
    color: color-mix(in srgb, #ce9178 75%, var(--text-primary));
  }

  :global(.token-comment) {
    color: color-mix(in srgb, #6a9955 70%, var(--text-primary));
    font-style: italic;
  }

  :global(.token-number) {
    color: color-mix(in srgb, #b5cea8 65%, var(--text-primary));
  }

  :global(.token-key) {
    color: color-mix(in srgb, #9cdcfe 65%, var(--text-primary));
  }
</style>
