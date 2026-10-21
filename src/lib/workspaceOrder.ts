// Ordinamento dei workspace nella sidebar (trascinamento): logica pura.

/** Sposta l'elemento in `from` alla posizione `to` (gli altri scorrono). */
export function moveItem<T>(items: T[], from: number, to: number): T[] {
  if (from === to || from < 0 || to < 0 || from >= items.length || to >= items.length) {
    return items;
  }
  const next = items.slice();
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item);
  return next;
}

export type DropPosition = "before" | "after";

/**
 * Nuovo ordine dopo aver rilasciato `dragId` prima/dopo `targetId`.
 * Id sconosciuti, rilascio su se stesso o posizione invariata: stessa istanza.
 */
export function dropOrder(
  ids: string[],
  dragId: string,
  targetId: string,
  position: DropPosition,
): string[] {
  const from = ids.indexOf(dragId);
  if (from < 0 || dragId === targetId || !ids.includes(targetId)) return ids;
  const rest = ids.filter((id) => id !== dragId);
  const at = rest.indexOf(targetId) + (position === "after" ? 1 : 0);
  const next = [...rest.slice(0, at), dragId, ...rest.slice(at)];
  return next.every((id, i) => id === ids[i]) ? ids : next;
}

/** Posizione del rilascio rispetto alla riga: metà superiore = prima. */
export function dropPositionAt(clientY: number, top: number, height: number): DropPosition {
  return clientY < top + height / 2 ? "before" : "after";
}
