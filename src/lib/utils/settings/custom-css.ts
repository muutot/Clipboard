/** UTF-16 code units, matching textarea maxlength and backend validation. */
export const MAX_CUSTOM_CSS_LENGTH = 65_536;
const STYLE_ID = "clipboard-custom-css";
const LAYER = "clipboard-custom-overrides";

export function normalizeCustomCss(value: unknown): string {
  return typeof value === "string" && value.length <= MAX_CUSTOM_CSS_LENGTH ? value : "";
}

/** Parse with the browser, preserving strings, nesting, conditional rules and keyframes. */
export function compileCustomCss(source: string, doc: Document = document): string {
  if (source.length > MAX_CUSTOM_CSS_LENGTH) throw new Error("CSS exceeds 65,536 characters");
  if (!source.trim()) return "";
  // A document without a browsing context never loads CSS resources while parsing.
  const parserDocument = doc.implementation.createHTMLDocument("");
  const parser = parserDocument.createElement("style");
  parser.media = "not all";
  // Constructed sheets ignore @import; the final override layer also excludes it.
  let sheet: CSSStyleSheet;
  try {
    const Sheet = doc.defaultView?.CSSStyleSheet;
    if (Sheet && "replaceSync" in Sheet.prototype) {
      sheet = new Sheet();
      sheet.replaceSync(source);
    } else {
      parser.textContent = source;
      parserDocument.head.append(parser);
      if (!parser.sheet) throw new Error("CSS stylesheet is unavailable");
      sheet = parser.sheet;
    }
    const visit = (rules: CSSRuleList) => {
      for (const rule of Array.from(rules)) {
        // !important is invalid inside keyframes and font-face descriptors.
        if (rule.type === 7) continue;
        // Nested declaration blocks have no dedicated CSSRule numeric constant.
        if (rule.type === 1 || rule.constructor.name === "CSSNestedDeclarations") {
          const style = (rule as CSSStyleRule | CSSNestedDeclarations).style;
          // Changing one priority can alter a shorthand's serialized value.
          const declarations = Array.from(
            style,
            (property) => [property, style.getPropertyValue(property)] as const,
          );
          for (const [property, value] of declarations) {
            style.setProperty(property, value, "important");
          }
        }
        if ("cssRules" in rule) visit((rule as CSSGroupingRule).cssRules);
      }
    };
    visit(sheet.cssRules);
    const rules = Array.from(sheet.cssRules)
      .filter((rule) => rule.type !== 3 && rule.type !== 10) // imports/namespaces cannot live in a layer
      .map((rule) => rule.cssText)
      .join("\n");
    return rules ? `@layer ${LAYER} {\n${rules}\n}` : "";
  } finally {
    parser.remove();
  }
}

let lastSource: string | undefined;
let lastCompiled = "";

export function applyCustomCss(source: string, enabled: boolean): void {
  if (typeof document === "undefined") return;
  const previous = document.getElementById(STYLE_ID);
  if (!enabled || !source.trim()) {
    previous?.remove();
    return;
  }
  try {
    if (source !== lastSource) {
      lastCompiled = compileCustomCss(source);
      lastSource = source;
    }
    if (!lastCompiled) {
      previous?.remove();
      return;
    }
    const style = previous ?? document.createElement("style");
    style.id = STYLE_ID;
    if (style.textContent !== lastCompiled) style.textContent = lastCompiled;
    if (!style.isConnected) document.head.append(style);
  } catch {
    // Keep the last working sheet; never log the user's stylesheet.
    console.error("Unable to apply custom CSS");
  }
}

export function isCustomCssRecoveryShortcut(event: KeyboardEvent): boolean {
  return (
    (event.ctrlKey || event.metaKey) && event.shiftKey && !event.altKey && event.code === "F12"
  );
}
