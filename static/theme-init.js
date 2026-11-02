// Synchronous first-paint hint; hydrated settings remain authoritative.
// Keep these preset variables and cache keys aligned with utils/settings/theme.ts.
(function () {
  var light = {
    "--bg-app": "#f5f5f5",
    "--bg-settings": "#f7f8fa",
    "--accent": "#1a6ae8",
    "--text-primary": "#1a1a1a",
    "--text-muted": "#666666",
    "--border-color": "#cccccc",
    "--card-bg": "#ffffff",
    "--surface-bg": "#ffffff",
    "--statusbar-bg": "#e8e8e8",
    "--hover-bg": "#e0e0e0",
    "--input-bg": "#f0f0f0",
    "--text-secondary": "#444444",
    "--text-faint": "#999999",
    "--placeholder-color": "#aaaaaa",
    "--border-subtle": "#dddddd",
    "--selection-color": "#1a73e8",
    "--success-color": "#388e3c",
    "--danger-color": "#d32f2f",
    "--warning-color": "#f9a825",
    "--scrollbar-color": "#aaaaaa",
  };
  var dark = {
    "--bg-app": "#1b1b1b",
    "--bg-settings": "#1b1b1b",
    "--accent": "#2576f8",
    "--text-primary": "#f5f5f5",
    "--text-muted": "#999999",
    "--border-color": "#3a3a3a",
    "--card-bg": "#1e1e1e",
    "--surface-bg": "#1e1e1e",
    "--statusbar-bg": "#181818",
    "--hover-bg": "#2c2c2c",
    "--input-bg": "#1a1a1a",
    "--text-secondary": "#b2b2b2",
    "--text-faint": "#6e6e6e",
    "--placeholder-color": "#6e6e6e",
    "--border-subtle": "#292929",
    "--selection-color": "#4c8dff",
    "--success-color": "#51b96b",
    "--danger-color": "#e85d5d",
    "--warning-color": "#e2c05d",
    "--scrollbar-color": "#858585",
  };
  var vars;
  var mode;
  try {
    mode = window.localStorage.getItem("clipboard-theme-mode");
    if (["system", "dark", "light", "custom"].indexOf(mode) === -1) mode = undefined;
    var raw = window.localStorage.getItem("clipboard-theme-vars");
    // A legacy palette has no mode: retain it until settings hydrate.
    if (raw && mode !== "system") vars = JSON.parse(raw);
  } catch (error) {
    // Storage can be blocked or corrupt; still honor the system preference.
  }
  var prefersDark =
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches;
  var fallback =
    mode === "dark" || mode === "custom" || ((!mode || mode === "system") && prefersDark)
      ? dark
      : light;
  if (!vars || typeof vars !== "object" || Array.isArray(vars)) vars = fallback;
  var root = document.documentElement;
  for (var name in light) {
    var color =
      typeof vars[name] === "string" && /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/.test(vars[name])
        ? vars[name]
        : fallback[name];
    root.style.setProperty(name, color);
  }
  var bg = root.style.getPropertyValue("--bg-app");
  var lightBackground =
    typeof bg === "string" &&
    /^#[0-9a-fA-F]{6}/.test(bg) &&
    (0.2126 * parseInt(bg.slice(1, 3), 16) +
      0.7152 * parseInt(bg.slice(3, 5), 16) +
      0.0722 * parseInt(bg.slice(5, 7), 16)) /
      255 >
      0.55;
  var darkScheme =
    mode === "dark" ||
    (mode === "custom" || !mode ? !lightBackground : mode === "system" && prefersDark);
  root.style.colorScheme = darkScheme ? "dark" : "light";
})();
