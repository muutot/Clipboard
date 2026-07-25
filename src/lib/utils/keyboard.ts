export function isEditableKeyboardTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;

  return (
    target.matches("input, textarea, select") ||
    target.isContentEditable ||
    target.closest("[contenteditable='true']") !== null
  );
}
