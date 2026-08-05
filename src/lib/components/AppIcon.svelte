<script lang="ts">
  export type IconName =
    | "grid"
    | "text"
    | "link"
    | "image"
    | "file"
    | "star"
    | "trash"
    | "restore"
    | "settings"
    | "pin"
    | "help"
    | "copy"
    | "download"
    | "search"
    | "clipboard"
    | "keyboard"
    | "filter"
    | "mail"
    | "phone"
    | "globe"
    | "palette"
    | "check"
    | "calendar"
    | "x"
    | "chevron-down"
    | "chevron-left"
    | "eye"
    | "edit"
    | "code"
    | "bar-chart"
    | "sliders"
    | "type"
    | "copy-plus"
    | "info"
    | "clock"
    | "mime"
    | "maximize"
    | "window-top"
    | "ruler"
    | "scan"
    | "pause"
    | "tag"
    | "plus"
    | "arrow-up"
    | "arrow-right";

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
  {:else if name === "pause"}
    <path d="M8 5v14" />
    <path d="M16 5v14" />
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
  {:else if name === "restore"}
    <path d="M3 7v6h6" />
    <path d="M21 17a9 9 0 0 0-9-9 9 9 0 0 0-6 3.3L3 13" />
    <path d="M17 17v6h-6" />
  {:else if name === "settings"}
    <circle cx="12" cy="12" r="3" />
    <path
      d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1A1.7 1.7 0 0 0 9 4.6 1.7 1.7 0 0 0 10 3V2.8h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z"
    />
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
  {:else if name === "keyboard"}
    <rect x="3" y="5" width="18" height="14" rx="2" />
    <path d="M7 9h.01M11 9h.01M15 9h.01M18 9h.01" />
    <path d="M6 13h.01M9 13h.01M12 13h.01M15 13h.01M18 13h.01" />
    <path d="M8 16h8" />
  {:else if name === "filter"}
    <path d="M4 5h16l-6 7v6l-4 2v-8z" />
  {:else if name === "mail"}
    <rect x="2" y="4" width="20" height="16" rx="2" />
    <path d="m22 4-10 8L2 4" />
  {:else if name === "phone"}
    <path
      d="M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6A19.79 19.79 0 0 1 2.12 4.18 2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72c.127.96.362 1.903.7 2.81a2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45c.907.338 1.85.573 2.81.7A2 2 0 0 1 22 16.92z"
    />
  {:else if name === "globe"}
    <circle cx="12" cy="12" r="9" />
    <path d="M2 12h20M12 3a15.3 15.3 0 0 1 0 18" />
    <path d="M12 3a15.3 15.3 0 0 0 0 18" />
  {:else if name === "palette"}
    <path d="M12 2a10 10 0 1 0 0 20v-2a8 8 0 1 1 8-8" />
    <circle cx="12" cy="12" r="3" />
    <path d="M8.5 8.5h.01M15.5 8.5h.01M9.5 14.5h.01" />
  {:else if name === "check"}
    <path d="M5 12l5 5L20 7" />
  {:else if name === "calendar"}
    <rect x="3" y="4" width="18" height="18" rx="2" />
    <path d="M16 2v4M8 2v4M3 10h18" />
  {:else if name === "x"}
    <path d="M18 6 6 18M6 6l12 12" />
  {:else if name === "chevron-down"}
    <path d="m6 9 6 6 6-6" />
  {:else if name === "chevron-left"}
    <path d="m15 18-6-6 6-6" />
  {:else if name === "eye"}
    <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
    <circle cx="12" cy="12" r="3" />
  {:else if name === "edit"}
    <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
    <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
  {:else if name === "code"}
    <path d="m16 18 6-6-6-6" />
    <path d="m8 6-6 6 6 6" />
  {:else if name === "bar-chart"}
    <path d="M18 20V10" />
    <path d="M12 20V4" />
    <path d="M6 20v-6" />
  {:else if name === "sliders"}
    <path d="M4 21v-7" />
    <path d="M4 10V3" />
    <path d="M12 21v-9" />
    <path d="M12 8V3" />
    <path d="M20 21v-5" />
    <path d="M20 12V3" />
    <path d="M1 14h6" />
    <path d="M9 8h6" />
    <path d="M17 16h6" />
  {:else if name === "type"}
    <path d="M5 4h14" />
    <path d="M9 20h6" />
    <path d="M12 4v16" />
    <path d="M5 8h4M15 8h4" />
  {:else if name === "copy-plus"}
    <rect x="8" y="8" width="12" height="12" rx="2" />
    <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
    <path d="M12 14h4M14 12v4" />
  {:else if name === "info"}
    <circle cx="12" cy="12" r="9" />
    <path d="M12 16v-4" />
    <path d="M12 8h.01" />
  {:else if name === "clock"}
    <circle cx="12" cy="12" r="9" />
    <path d="M12 6v6l4 2" />
  {:else if name === "mime"}
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <path d="M14 2v6h6" />
    <path d="M9 13l2 2 4-4" />
  {:else if name === "maximize"}
    <path d="M8 3H5a2 2 0 0 0-2 2v3" />
    <path d="M21 8V5a2 2 0 0 0-2-2h-3" />
    <path d="M3 16v3a2 2 0 0 0 2 2h3" />
    <path d="M16 21h3a2 2 0 0 0 2-2v-3" />
  {:else if name === "window-top"}
    <rect x="2" y="4" width="15" height="13" rx="2" opacity="0.35" />
    <rect x="7" y="7" width="15" height="13" rx="2" />
  {:else if name === "ruler"}
    <path
      d="M21.3 15.3a2.4 2.4 0 0 1 0 3.4l-2.6 2.6a2.4 2.4 0 0 1-3.4 0L2.7 8.7a2.41 2.41 0 0 1 0-3.4l2.6-2.6a2.41 2.41 0 0 1 3.4 0Z"
    />
    <path d="m14.5 12.5 2-2" />
    <path d="m11.5 9.5 2-2" />
    <path d="m8.5 6.5 2-2" />
    <path d="m17.5 15.5 2-2" />
  {:else if name === "scan"}
    <path d="M3 7V5a2 2 0 0 1 2-2h2" />
    <path d="M17 3h2a2 2 0 0 1 2 2v2" />
    <path d="M21 17v2a2 2 0 0 1-2 2h-2" />
    <path d="M7 21H5a2 2 0 0 1-2-2v-2" />
    <circle cx="12" cy="12" r="1" />
  {:else if name === "tag"}
    <path
      d="M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z"
    />
    <circle cx="7.5" cy="7.5" r="1.25" />
  {:else if name === "plus"}
    <path d="M12 5v14" />
    <path d="M5 12h14" />
  {:else if name === "arrow-up"}
    <path d="M12 19V5" />
    <path d="m6 11 6-6 6 6" />
  {:else if name === "arrow-right"}
    <path d="M5 12h14" />
    <path d="m13 6 6 6-6 6" />
  {/if}
</svg>
