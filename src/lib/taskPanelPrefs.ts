// Preferenze del pannello Task: stato compresso/espanso per workspace (e per la
// vista Globale), ricordato in localStorage. Funzioni pure a zero dipendenze,
// importabili sia dall'app sia dai test `node --test`: lo storage è iniettato.

/** Chiave di localStorage: oggetto JSON `{ [workspaceId | "__global__"]: true }`. */
export const TASK_PANEL_STORAGE_KEY = "cli-companion.task-panel.collapsed";

/** Parte minima di `Storage` che serve qui. */
export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export type CollapsedMap = Record<string, boolean>;

/** Legge la mappa; storage assente, vuoto, illeggibile o malformato = tutto espanso. */
export function readCollapsed(storage: StorageLike | null | undefined): CollapsedMap {
  try {
    const raw = storage?.getItem(TASK_PANEL_STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return {};
    const out: CollapsedMap = {};
    for (const [k, v] of Object.entries(parsed)) if (v === true) out[k] = true;
    return out;
  } catch {
    return {};
  }
}

/** Scrive la mappa (solo i compressi); gli errori di storage sono ignorati. */
export function writeCollapsed(storage: StorageLike | null | undefined, map: CollapsedMap): void {
  try {
    storage?.setItem(TASK_PANEL_STORAGE_KEY, JSON.stringify(map));
  } catch {
    // Storage non disponibile (finestra privata, quota): il pannello resta espanso al riavvio.
  }
}

/** Nuova mappa con `id` compresso o espanso (gli espansi non si salvano: è il default). */
export function withCollapsed(map: CollapsedMap, id: string, collapsed: boolean): CollapsedMap {
  const next = { ...map };
  if (collapsed) next[id] = true;
  else delete next[id];
  return next;
}
