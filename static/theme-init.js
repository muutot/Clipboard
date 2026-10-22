// First-paint theme hint. Loaded synchronously from <head> (see src/app.html),
// before the browser paints the dark defaults declared in app.html/app.css, so
// a light theme never flashes a dark surface in the main, settings, or float
// window. The snapshot is written by applyThemeColors
// (src/lib/utils/settings/theme.ts, THEME_CACHE_KEY) and is never the source of
// truth: the hydrated settings store re-applies the real colors afterwards.
(function () {
  try {
    var raw = window.localStorage.getItem("clipboard-theme-vars");
    if (!raw) return;
    var vars = JSON.parse(raw);
    if (!vars || typeof vars !== "object") return;
    var root = document.documentElement;
    for (var name in vars) {
      if (Object.prototype.hasOwnProperty.call(vars, name) && typeof vars[name] === "string") {
        root.style.setProperty(name, vars[name]);
      }
    }
  } catch (error) {
    // Missing or corrupt snapshot: keep the built-in defaults.
  }
})();
