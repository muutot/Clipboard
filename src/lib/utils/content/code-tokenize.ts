/**
 * Shared tokenizer for code detail views, backed by highlight.js.
 *
 * Used by `CodePreview.svelte` (read-only highlight) and `CodeEditor.svelte`
 * (live highlight backdrop while editing). Runs synchronously on every
 * keystroke while editing, so the `common` language bundle and the per-line
 * split below avoid any async grammar loading.
 */

import hljs from "highlight.js/lib/common";

export interface CodeToken {
  text: string;
  classes: string;
}

/** Display-language names (dropdown labels, CODE_PATTERNS hits) → hljs ids. */
const NAME_TO_HLJS_ID: Record<string, string> = {
  text: "plaintext",
  markdown: "markdown",
  JavaScript: "javascript",
  TypeScript: "typescript",
  JSX: "javascript",
  Python: "python",
  Rust: "rust",
  SQL: "sql",
  HTML: "xml",
  CSS: "css",
  JSON: "json",
  Shell: "bash",
  YAML: "yaml",
  Go: "go",
  Java: "java",
  C: "c",
  "C++": "cpp",
  "C#": "csharp",
};

export function detectCodeLang(text: string): string {
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

function plainLines(text: string): CodeToken[][] {
  return text.split("\n").map((line) => (line ? [{ text: line, classes: "" }] : []));
}

/** hljs output is entity-escaped; tokens carry raw text, so decode it back. */
function decodeEntities(text: string): string {
  return text
    .replaceAll("&amp;", "&")
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&quot;", '"')
    .replaceAll("&#39;", "'");
}

/**
 * Splits hljs HTML output into per-line token arrays. hljs spans routinely
 * cross line breaks (template literals, block comments), so the splitter
 * tracks the stack of open span classes and re-opens them on the next line.
 */
function splitHtmlLines(html: string): CodeToken[][] {
  const lines: CodeToken[][] = [];
  let current: CodeToken[] = [];
  let buffer = "";
  const openClasses: string[] = [];

  const flushText = () => {
    if (buffer) {
      current.push({ text: buffer, classes: openClasses.join(" ") });
      buffer = "";
    }
  };
  const pushLine = () => {
    flushText();
    lines.push(current);
    current = [];
  };

  // split() with a capture group keeps the tag delimiters at odd indices.
  const parts = html.split(/(<span class="[^"]*">|<\/span>)/);
  for (const part of parts) {
    if (!part) continue;
    if (part === "</span>") {
      flushText();
      openClasses.pop();
    } else if (part.startsWith('<span class="')) {
      flushText();
      openClasses.push(part.slice('<span class="'.length, -2));
    } else {
      const segments = part.split("\n");
      for (let index = 0; index < segments.length; index++) {
        if (index > 0) pushLine();
        buffer += decodeEntities(segments[index]);
      }
    }
  }
  flushText();
  // The final line never sees a "\n" boundary, so it must be flushed here —
  // without this, single-line content rendered zero lines and content without
  // a trailing newline silently lost its last line. A trailing newline still
  // yields no extra empty line, matching the editor's lineCount convention.
  if (current.length > 0 || lines.length === 0) {
    lines.push(current);
  }
  return lines;
}

export function tokenizeCode(text: string, lang: string): CodeToken[][] {
  const id = NAME_TO_HLJS_ID[lang] ?? lang;
  if (!hljs.getLanguage(id)) return plainLines(text);

  try {
    return splitHtmlLines(hljs.highlight(text, { language: id, ignoreIllegals: true }).value);
  } catch {
    // A grammar crash must never take the editor down with it.
    return plainLines(text);
  }
}
