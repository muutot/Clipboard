const ELLIPSIS = "\u2026";

export function alignDropdownOptionText(container: HTMLElement): void {
  let anyOverflow = false;
  for (const button of container.querySelectorAll<HTMLElement>("button")) {
    const probe = button.querySelector<HTMLElement>(":scope > span") ?? button;
    if (probe.clientWidth > 0 && probe.scrollWidth > probe.clientWidth + 1) {
      anyOverflow = true;
      truncateToFit(button, probe);
    }
  }
  container.classList.toggle("text-overflow", anyOverflow);
}

function truncateToFit(button: HTMLElement, probe: HTMLElement): void {
  const full = probe.textContent ?? "";
  const chars = Array.from(full);
  if (chars.length <= 1) return;
  button.title = full;
  probe.textContent = full + ELLIPSIS;
  let keep = chars.length;
  while (probe.scrollWidth > probe.clientWidth + 1 && keep > 1) {
    keep -= 1;
    probe.textContent = chars.slice(0, keep).join("") + ELLIPSIS;
  }
}
