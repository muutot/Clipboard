<script lang="ts">
  import { parseMarkdown } from "$lib/utils/markdown";

  interface Props {
    content: string;
  }

  let { content }: Props = $props();

  const html = $derived(parseMarkdown(content));
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
    font:
      11px/1.55 "Cascadia Code",
      Consolas,
      monospace;
    overflow-x: auto;
  }

  :global(.markdown-body .md-inline-code) {
    padding: 2px 5px;
    border: 1px solid #333;
    border-radius: 4px;
    color: #ce9178;
    background: #1a1a1a;
    font:
      11px "Cascadia Code",
      Consolas,
      monospace;
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
