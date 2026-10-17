// Gruppi di terminali per workspace e griglia dei pannelli. Funzioni pure a
// zero dipendenze: importabili sia dall'app sia dai test `node --test`
// (type-stripping), come src/lib/terminal.ts.

/** Rettangolo in percentuale dell'area del gruppo. */
export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Colonne e righe della griglia automatica per `n` pannelli. */
export function gridDims(n: number): { cols: number; rows: number } {
  if (!Number.isFinite(n) || n < 1) return { cols: 0, rows: 0 };
  const count = Math.floor(n);
  const cols = Math.ceil(Math.sqrt(count));
  return { cols, rows: Math.ceil(count / cols) };
}

/**
 * Griglia automatica: cols = ceil(sqrt(n)), rows = ceil(n / cols). I pannelli
 * dell'ultima riga, se incompleta, si allargano a riempire tutta la larghezza.
 * Ordine dei rettangoli: righe dall'alto, da sinistra a destra.
 */
export function gridRects(n: number): Rect[] {
  const { cols, rows } = gridDims(n);
  const count = cols === 0 ? 0 : Math.floor(n);
  const rects: Rect[] = [];
  for (let i = 0; i < count; i++) {
    const row = Math.floor(i / cols);
    const inRow = row === rows - 1 ? count - cols * (rows - 1) : cols;
    const col = i - row * cols;
    rects.push({
      left: (col * 100) / inRow,
      top: (row * 100) / rows,
      width: 100 / inRow,
      height: 100 / rows,
    });
  }
  return rects;
}

/**
 * Sposta `fromId` nella posizione occupata da `toId` (gli altri scorrono).
 * Id assenti o uguali: ordine invariato (stessa istanza).
 */
export function reorderIds(order: string[], fromId: string, toId: string): string[] {
  const from = order.indexOf(fromId);
  const to = order.indexOf(toId);
  if (from < 0 || to < 0 || from === to) return order;
  const next = order.slice();
  next.splice(from, 1);
  next.splice(to, 0, fromId);
  return next;
}

// ---------------------------------------------------------------------------
// Stato dei gruppi
// ---------------------------------------------------------------------------

/** Gruppo di terminali di un workspace. */
export interface GroupState {
  workspaceId: string;
  /** Ordine dei pannelli nella griglia. */
  order: string[];
  activeTabId: string | null;
  maximizedTabId: string | null;
}

export interface LayoutState {
  /** Nell'ordine in cui i gruppi sono stati aperti la prima volta. */
  groups: GroupState[];
  activeWorkspaceId: string | null;
}

export const EMPTY_LAYOUT: LayoutState = { groups: [], activeWorkspaceId: null };

function mapGroup(
  state: LayoutState,
  workspaceId: string,
  fn: (g: GroupState) => GroupState,
): LayoutState {
  return {
    ...state,
    groups: state.groups.map((g) => (g.workspaceId === workspaceId ? fn(g) : g)),
  };
}

export function findGroupOfTab(state: LayoutState, tabId: string): GroupState | undefined {
  return state.groups.find((g) => g.order.includes(tabId));
}

/**
 * Apre un pannello: crea il gruppo del workspace se manca (in coda), lo
 * accoda, lo rende attivo e attiva il gruppo. Un pannello massimizzato torna
 * normale, altrimenti il nuovo non sarebbe visibile.
 */
export function openTab(state: LayoutState, workspaceId: string, tabId: string): LayoutState {
  if (findGroupOfTab(state, tabId)) return state;
  const exists = state.groups.some((g) => g.workspaceId === workspaceId);
  const groups = exists
    ? state.groups.map((g) =>
        g.workspaceId === workspaceId
          ? { ...g, order: [...g.order, tabId], activeTabId: tabId, maximizedTabId: null }
          : g,
      )
    : [
        ...state.groups,
        { workspaceId, order: [tabId], activeTabId: tabId, maximizedTabId: null },
      ];
  return { groups, activeWorkspaceId: workspaceId };
}

/**
 * Chiude un pannello. Il gruppo che resta vuoto sparisce; se era quello
 * attivo passa al gruppo adiacente (quello che ne prende il posto, o il
 * precedente se era l'ultimo). Il pannello attivo di un gruppo che perde
 * quello attivo diventa il vicino.
 */
export function closeTabIn(state: LayoutState, tabId: string): LayoutState {
  const gi = state.groups.findIndex((g) => g.order.includes(tabId));
  if (gi < 0) return state;
  const group = state.groups[gi];
  const idx = group.order.indexOf(tabId);
  const order = group.order.filter((id) => id !== tabId);

  if (order.length === 0) {
    const groups = state.groups.filter((_, i) => i !== gi);
    const activeWorkspaceId =
      state.activeWorkspaceId === group.workspaceId
        ? (groups[Math.min(gi, groups.length - 1)]?.workspaceId ?? null)
        : state.activeWorkspaceId;
    return { groups, activeWorkspaceId };
  }

  const activeTabId =
    group.activeTabId === tabId
      ? order[Math.min(idx, order.length - 1)]
      : group.activeTabId;
  const maximizedTabId = group.maximizedTabId === tabId ? null : group.maximizedTabId;
  return mapGroup(state, group.workspaceId, (g) => ({
    ...g,
    order,
    activeTabId,
    maximizedTabId,
  }));
}

/** Attiva il gruppo di un workspace (se esiste). */
export function selectGroup(state: LayoutState, workspaceId: string): LayoutState {
  if (state.activeWorkspaceId === workspaceId) return state;
  if (!state.groups.some((g) => g.workspaceId === workspaceId)) return state;
  return { ...state, activeWorkspaceId: workspaceId };
}

/** Rende attivo un pannello (e il suo gruppo). */
export function setActivePane(state: LayoutState, tabId: string): LayoutState {
  const group = findGroupOfTab(state, tabId);
  if (!group) return state;
  if (group.activeTabId === tabId && state.activeWorkspaceId === group.workspaceId) {
    return state;
  }
  return {
    ...mapGroup(state, group.workspaceId, (g) => ({ ...g, activeTabId: tabId })),
    activeWorkspaceId: group.workspaceId,
  };
}

/** Massimizza il pannello (o lo ripristina se lo e' gia'). */
export function toggleMaximize(state: LayoutState, tabId: string): LayoutState {
  const group = findGroupOfTab(state, tabId);
  if (!group) return state;
  return {
    ...mapGroup(state, group.workspaceId, (g) => ({
      ...g,
      activeTabId: tabId,
      maximizedTabId: g.maximizedTabId === tabId ? null : tabId,
    })),
    activeWorkspaceId: group.workspaceId,
  };
}

/** Sposta `fromId` sulla posizione di `toId`, solo se sono dello stesso gruppo. */
export function reorderTabs(state: LayoutState, fromId: string, toId: string): LayoutState {
  const group = findGroupOfTab(state, fromId);
  if (!group || !group.order.includes(toId)) return state;
  const order = reorderIds(group.order, fromId, toId);
  if (order === group.order) return state;
  return mapGroup(state, group.workspaceId, (g) => ({ ...g, order }));
}

/**
 * Rettangolo di ogni pannello del gruppo. Con un pannello massimizzato quello
 * occupa tutta l'area e gli altri non compaiono nel risultato (nascosti).
 */
export function paneRects(group: GroupState): Record<string, Rect> {
  const out: Record<string, Rect> = {};
  if (group.maximizedTabId && group.order.includes(group.maximizedTabId)) {
    out[group.maximizedTabId] = { left: 0, top: 0, width: 100, height: 100 };
    return out;
  }
  const rects = gridRects(group.order.length);
  group.order.forEach((id, i) => {
    out[id] = rects[i];
  });
  return out;
}

export interface PaneLayout {
  rect: Rect | null;
  /** Il pannello e' mostrato (gruppo attivo e non nascosto da un maximize). */
  visible: boolean;
  /** Il pannello riceve la tastiera. */
  focused: boolean;
  maximized: boolean;
  groupWorkspaceId: string;
}

/** Layout di tutti i pannelli, indicizzato per id. */
export function computePaneLayouts(state: LayoutState): Record<string, PaneLayout> {
  const out: Record<string, PaneLayout> = {};
  for (const g of state.groups) {
    const rects = paneRects(g);
    const groupActive = g.workspaceId === state.activeWorkspaceId;
    for (const id of g.order) {
      const rect = rects[id] ?? null;
      const visible = groupActive && rect !== null;
      out[id] = {
        rect,
        visible,
        focused: visible && g.activeTabId === id,
        maximized: g.maximizedTabId === id,
        groupWorkspaceId: g.workspaceId,
      };
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// Stato aggregato di un gruppo
// ---------------------------------------------------------------------------

/** Stato mostrato su un gruppo: ❗ > ✓ > in corso (pallino giallo) > nulla. */
export type GroupIndicator = "alert" | "check" | "busy" | "none";

export function groupIndicator(
  statuses: Array<{ icon: "dot" | "check" | "alert"; tone: string }>,
): GroupIndicator {
  if (statuses.some((s) => s.icon === "alert")) return "alert";
  if (statuses.some((s) => s.icon === "check")) return "check";
  if (statuses.some((s) => s.tone === "busy")) return "busy";
  return "none";
}
