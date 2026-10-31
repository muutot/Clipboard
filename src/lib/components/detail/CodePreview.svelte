<script lang="ts">
  interface Props {
    content: string;
    language?: string | null;
  }

  import { FILE_PREVIEW_LIMIT } from "$lib/utils/content/file-preview";
  import { detectCodeLang, tokenizeCode } from "$lib/utils/content/code-tokenize";

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

  const detectedLang = $derived(language || detectCodeLang(preview.text));

  const tokenizedLines = $derived(tokenizeCode(preview.text, detectedLang));
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
