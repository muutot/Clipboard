// Maps each selected id to its 1-based position in selection order. The
// multi-selection doubles as the paste queue (see the bulk bar's ordered
// paste), so the cards need that order visible; a `Set` preserves insertion
// order and this keeps the derivation in one testable place. Contains no IPC,
// stores, or DOM.

/** Assigns 1-based paste positions in selection (insertion) order. */
export function selectionOrder(selectedIds: Iterable<string>): Map<string, number> {
  const order = new Map<string, number>();
  let position = 1;
  for (const id of selectedIds) {
    if (!order.has(id)) order.set(id, position);
    position += 1;
  }
  return order;
}
