<script lang="ts">
  export type IconName =
    | "grid"
    | "text"
    | "link"
    | "image"
    | "file"
    | "star"
    | "trash"
    | "settings"
    | "pin"
    | "help"
    | "copy"
    | "download"
    | "search"
    | "clipboard";

  interface Props {
    name: IconName;
    size?: number;
    strokeWidth?: number;
    filled?: boolean;
  }

  let { name, size = 18, strokeWidth = 1.8, filled = false }: Props = $props();
</script>

<svg
  xmlns="http://www.w3.org/2000/svg"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={filled ? "currentColor" : "none"}
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === "grid"}
    <rect x="3" y="3" width="7" height="7" rx="1" />
    <rect x="14" y="3" width="7" height="7" rx="1" />
    <rect x="3" y="14" width="7" height="7" rx="1" />
    <rect x="14" y="14" width="7" height="7" rx="1" />
  {:else if name === "text"}
    <path d="M5 4h14" />
    <path d="M9 20h6" />
    <path d="M12 4v16" />
  {:else if name === "link"}
    <path d="M10 13a5 5 0 0 0 7.1.1l2-2a5 5 0 0 0-7.1-7.1l-1.1 1.1" />
    <path d="M14 11a5 5 0 0 0-7.1-.1l-2 2a5 5 0 0 0 7.1 7.1l1.1-1.1" />
  {:else if name === "image"}
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <circle cx="8.5" cy="9" r="1.5" />
    <path d="m21 15-5-5L5 20" />
  {:else if name === "file"}
    <path d="M6 2h8l4 4v16H6z" />
    <path d="M14 2v5h5" />
  {:else if name === "star"}
    <path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9z" />
  {:else if name === "trash"}
    <path d="M4 7h16" />
    <path d="M9 7V4h6v3" />
    <path d="m7 7 1 14h8l1-14" />
    <path d="M10 11v6M14 11v6" />
  {:else if name === "settings"}
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1A1.7 1.7 0 0 0 9 4.6 1.7 1.7 0 0 0 10 3V2.8h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z" />
  {:else if name === "pin"}
    <path d="m14 4 6 6-3 1-4 4-1 5-2-2-2-2 5-1 4-4z" />
    <path d="m5 19 4-4" />
  {:else if name === "help"}
    <circle cx="12" cy="12" r="9" />
    <path d="M9.8 9a2.4 2.4 0 1 1 3.1 2.3c-.9.4-.9 1-.9 1.7" />
    <path d="M12 17h.01" />
  {:else if name === "copy"}
    <rect x="8" y="8" width="12" height="12" rx="2" />
    <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
  {:else if name === "download"}
    <path d="M12 3v12" />
    <path d="m7 10 5 5 5-5" />
    <path d="M5 21h14" />
  {:else if name === "search"}
    <circle cx="11" cy="11" r="7" />
    <path d="m20 20-4-4" />
  {:else if name === "clipboard"}
    <rect x="5" y="4" width="14" height="17" rx="2" />
    <path d="M9 4.5V3h6v1.5" />
    <path d="M9 9h6M9 13h6M9 17h4" />
  {/if}
</svg>
