<script lang="ts">
  interface Props {
    content: string;
    language?: string | null;
  }

  let { content, language = null }: Props = $props();

  const detectedLang = $derived(language ?? detectLanguage(content));

  const tokenizedLines = $derived(tokenize(content, detectedLang));

  function detectLanguage(text: string): string {
    const patterns: { name: string; re: RegExp; weight: number }[] = [
      { name: "TypeScript", re: /^(import|export)\s|:\s*(string|number|boolean|void|any)\b|interface\s+\w+\s*\{|type\s+\w+\s*=/m, weight: 3 },
      { name: "Rust", re: /^(use\s+|fn\s+|let\s+mut\s|struct\s|impl\s|pub\s|mod\s)/m, weight: 3 },
      { name: "Python", re: /^(def\s+|class\s+\w+.*:|import\s+\w+|from\s+\w+\s+import|print\(|elif\s)/m, weight: 3 },
      { name: "JSON", re: /^\s*[{\[]\s*$|"[\w-]+"\s*:\s*|^\s*[}\]]\s*,?\s*$/m, weight: 2 },
      { name: "HTML", re: /^<!DOCTYPE|<html\b|<head\b|<body\b|<div\b|<span\b|<script\b|<style\b/m, weight: 3 },
      { name: "SQL", re: /^(SELECT|INSERT|UPDATE|DELETE|CREATE|ALTER|DROP)\s|^\s*(FROM|WHERE|JOIN|ORDER BY|GROUP BY)\s/mi, weight: 3 },
      { name: "Shell", re: /^#!\//m, weight: 4 },
      { name: "Shell", re: /^(echo |cd |ls |grep |mkdir |sudo |apt |npm |yarn |git |docker |export\s)/m, weight: 2 },
      { name: "CSS", re: /^[.#@][\w-]+\s*\{|^\s*(color|margin|padding|display|font-size|background|border):\s/m, weight: 2 },
      { name: "JavaScript", re: /^(function\s+\w+|var\s+|const\s+|let\s+|console\.|document\.|window\.|require\()/m, weight: 2 },
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
    JavaScript: new Set(["function","var","const","let","return","if","else","for","while","class","new","this","null","undefined","true","false","import","export","default","from","await","async","try","catch","throw","typeof","instanceof","of","in"]),
    TypeScript: new Set(["function","var","const","let","return","if","else","for","while","class","new","this","null","undefined","true","false","import","export","default","from","await","async","try","catch","throw","typeof","instanceof","of","in","interface","type","enum","implements","extends","as","is","readonly","private","public","protected"]),
    Rust: new Set(["fn","let","mut","struct","impl","pub","mod","use","match","if","else","for","while","loop","return","self","true","false","async","await","enum","trait","where","ref","move","unsafe","extern","crate","super","in","const","static","type","as"]),
    Python: new Set(["def","class","import","from","return","if","elif","else","for","while","try","except","finally","with","as","pass","break","continue","lambda","yield","raise","True","False","None","and","or","not","in","is","print","self","global","nonlocal","del","assert"]),
    SQL: new Set(["SELECT","FROM","WHERE","INSERT","UPDATE","DELETE","CREATE","ALTER","DROP","TABLE","INDEX","JOIN","INNER","LEFT","RIGHT","OUTER","ON","AND","OR","NOT","NULL","IS","IN","BETWEEN","LIKE","ORDER","BY","GROUP","HAVING","LIMIT","OFFSET","AS","SET","VALUES","INTO","DISTINCT","COUNT","SUM","AVG","MAX","MIN","EXISTS","CASE","WHEN","THEN","ELSE","END","UNION","ALL","PRIMARY","KEY","FOREIGN","REFERENCES","CONSTRAINT","DEFAULT","CHECK","UNIQUE","CASCADE","BEGIN","COMMIT","ROLLBACK"]),
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

      if (["JavaScript","TypeScript","Rust","CSS"].includes(lang)) {
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
      const re = /("[^"\\]*(?:\\.[^"\\]*)*"|'[^'\\]*(?:\\.[^'\\]*)*'|`[^`\\]*(?:\\.[^`\\]*)*`|[\w]+|[^\w\s]|\s+)/g;
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
    <pre class="code-content"><code>{#each tokenizedLines as line, i}<span class="code-line"><span class="line-number">{String(i + 1).padStart(String(tokenizedLines.length).length, " ")}</span>{#each line as token}{#if token.classes}<span class={token.classes}>{token.text}</span>{:else}{token.text}{/if}{/each}</span>
      {/each}</code></pre>
  </div>
{/if}

<style>
  .code-block {
    border: 1px solid #2e2e2e;
    border-radius: 8px;
    overflow: hidden;
    background: #141414;
  }

  .code-lang-label {
    display: inline-block;
    padding: 3px 10px;
    margin: 8px 10px 0;
    border: 1px solid #3a3a3a;
    border-radius: 5px;
    color: #888;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .code-content {
    margin: 0;
    padding: 8px 0;
    overflow-x: auto;
    font: 11px/1.35 "Cascadia Code", Consolas, "SFMono-Regular", monospace;
    color: #d4d4d4;
  }

  .code-content code {
    display: block;
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
    color: #555;
    user-select: none;
    flex-shrink: 0;
  }

  :global(.token-keyword) {
    color: #c586c0;
  }

  :global(.token-string) {
    color: #ce9178;
  }

  :global(.token-comment) {
    color: #6a9955;
    font-style: italic;
  }

  :global(.token-number) {
    color: #b5cea8;
  }

  :global(.token-key) {
    color: #9cdcfe;
  }

  @media (max-width: 520px) {
    .code-content {
      font-size: 10px;
    }
  }
</style>
