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

/** Id riservato della vista globale (mai un id di workspace reale). */
export const GLOBAL_ID = "__global__";

/** Una vista a griglia: ordine dei pannelli, pannello attivo e massimizzato. */
export interface ViewState {
  /** Ordine dei pannelli nella griglia. */
  order: string[];
  activeTabId: string | null;
  maximizedTabId: string | null;
}

/** Gruppo di terminali di un workspace. */
export interface GroupState extends ViewState {
  workspaceId: string;
}

export type ActiveView = "workspace" | "global";

export interface LayoutState {
  /** Nell'ordine in cui i gruppi sono stati aperti la prima volta. */
  groups: GroupState[];
  /** Gruppo selezionato (resta valido anche mentre e' mostrata la vista globale). */
  activeWorkspaceId: string | null;
  /**
   * Vista globale: i tab fissati dall'utente, di workspace diversi, con
   * ordine proprio. Ogni tab fissato resta anche nel suo gruppo.
   */
  global: ViewState;
  /** Quale vista e' mostrata. "global" solo se `global.order` non e' vuoto. */
  activeView: ActiveView;
}

export const EMPTY_GLOBAL: ViewState = { order: [], activeTabId: null, maximizedTabId: null };

export const EMPTY_LAYOUT: LayoutState = {
  groups: [],
  activeWorkspaceId: null,
  global: EMPTY_GLOBAL,
  activeView: "workspace",
};

/** Il pannello e' nella vista globale che e' anche quella mostrata. */
function inShownGlobal(state: LayoutState, tabId: string): boolean {
  return state.activeView === "global" && state.global.order.includes(tabId);
}

/** Pannello attivo della vista mostrata (per Ctrl+Maiusc+M). */
export function activePaneId(state: LayoutState): string | null {
  if (state.activeView === "global") return state.global.activeTabId;
  return state.groups.find((g) => g.workspaceId === state.activeWorkspaceId)?.activeTabId ?? null;
}

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
  return { ...state, groups, activeWorkspaceId: workspaceId, activeView: "workspace" };
}

/**
 * Chiude un pannello. Il gruppo che resta vuoto sparisce; se era quello
 * attivo passa al gruppo adiacente (quello che ne prende il posto, o il
 * precedente se era l'ultimo). Il pannello attivo di un gruppo che perde
 * quello attivo diventa il vicino. Il pannello esce anche dalla vista globale.
 */
export function closeTabIn(state: LayoutState, tabId: string): LayoutState {
  const closed = closeInGroup(state, tabId);
  return closed === state ? state : unpinTab(closed, tabId);
}

function closeInGroup(state: LayoutState, tabId: string): LayoutState {
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
    return { ...state, groups, activeWorkspaceId };
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
  if (!state.groups.some((g) => g.workspaceId === workspaceId)) return state;
  if (state.activeWorkspaceId === workspaceId && state.activeView === "workspace") return state;
  return { ...state, activeWorkspaceId: workspaceId, activeView: "workspace" };
}

/** Rende attivo un pannello (e il suo gruppo). */
export function setActivePane(state: LayoutState, tabId: string): LayoutState {
  if (inShownGlobal(state, tabId)) {
    if (state.global.activeTabId === tabId) return state;
    return { ...state, global: { ...state.global, activeTabId: tabId } };
  }
  const group = findGroupOfTab(state, tabId);
  if (!group) return state;
  if (
    group.activeTabId === tabId &&
    state.activeWorkspaceId === group.workspaceId &&
    state.activeView === "workspace"
  ) {
    return state;
  }
  return {
    ...mapGroup(state, group.workspaceId, (g) => ({ ...g, activeTabId: tabId })),
    activeWorkspaceId: group.workspaceId,
    activeView: "workspace",
  };
}

/**
 * Porta in primo piano un pannello (click su notifica): attiva il suo
 * workspace e il pannello, e toglie un eventuale maximize di un altro
 * pannello del gruppo. Va sempre al gruppo del workspace, anche se il tab e'
 * fissato nella vista globale. Tab inesistente: stato invariato (stessa istanza).
 */
export function focusTab(state: LayoutState, tabId: string): LayoutState {
  const group = findGroupOfTab(state, tabId);
  if (!group) return state;
  const hidden = group.maximizedTabId !== null && group.maximizedTabId !== tabId;
  if (
    !hidden &&
    group.activeTabId === tabId &&
    state.activeWorkspaceId === group.workspaceId &&
    state.activeView === "workspace"
  ) {
    return state;
  }
  return {
    ...mapGroup(state, group.workspaceId, (g) => ({
      ...g,
      activeTabId: tabId,
      maximizedTabId: hidden ? null : g.maximizedTabId,
    })),
    activeWorkspaceId: group.workspaceId,
    activeView: "workspace",
  };
}

/** Massimizza il pannello (o lo ripristina se lo e' gia'). */
export function toggleMaximize(state: LayoutState, tabId: string): LayoutState {
  if (inShownGlobal(state, tabId)) {
    const g = state.global;
    return {
      ...state,
      global: { ...g, activeTabId: tabId, maximizedTabId: g.maximizedTabId === tabId ? null : tabId },
    };
  }
  const group = findGroupOfTab(state, tabId);
  if (!group) return state;
  return {
    ...mapGroup(state, group.workspaceId, (g) => ({
      ...g,
      activeTabId: tabId,
      maximizedTabId: g.maximizedTabId === tabId ? null : tabId,
    })),
    activeWorkspaceId: group.workspaceId,
    activeView: "workspace",
  };
}

/**
 * Sposta `fromId` sulla posizione di `toId`, solo se sono dello stesso gruppo
 * (con la vista globale mostrata: solo entrambi nella vista globale).
 */
export function reorderTabs(state: LayoutState, fromId: string, toId: string): LayoutState {
  if (state.activeView === "global") {
    const order = reorderIds(state.global.order, fromId, toId);
    return order === state.global.order ? state : { ...state, global: { ...state.global, order } };
  }
  const group = findGroupOfTab(state, fromId);
  if (!group || !group.order.includes(toId)) return state;
  const order = reorderIds(group.order, fromId, toId);
  if (order === group.order) return state;
  return mapGroup(state, group.workspaceId, (g) => ({ ...g, order }));
}

/** Mostra la vista globale (solo se contiene almeno un pannello). */
export function selectGlobal(state: LayoutState): LayoutState {
  if (state.global.order.length === 0 || state.activeView === "global") return state;
  return { ...state, activeView: "global" };
}

/** Fissa un pannello nella vista globale, in coda. Gia' fissato o sconosciuto: invariato. */
export function pinTab(state: LayoutState, tabId: string): LayoutState {
  if (state.global.order.includes(tabId) || !findGroupOfTab(state, tabId)) return state;
  const g = state.global;
  return {
    ...state,
    global: { ...g, order: [...g.order, tabId], activeTabId: g.activeTabId ?? tabId },
  };
}

/**
 * Toglie un pannello dalla vista globale. Come in `closeTabIn`, il pannello
 * attivo diventa il vicino; una vista vuota non resta mostrata.
 */
export function unpinTab(state: LayoutState, tabId: string): LayoutState {
  const g = state.global;
  const idx = g.order.indexOf(tabId);
  if (idx < 0) return state;
  const order = g.order.filter((id) => id !== tabId);
  const global: ViewState = {
    order,
    activeTabId:
      g.activeTabId === tabId ? (order[Math.min(idx, order.length - 1)] ?? null) : g.activeTabId,
    maximizedTabId: g.maximizedTabId === tabId ? null : g.maximizedTabId,
  };
  return {
    ...state,
    global,
    activeView: order.length === 0 ? "workspace" : state.activeView,
  };
}

/** Fissa o toglie dalla vista globale. */
export function togglePin(state: LayoutState, tabId: string): LayoutState {
  return state.global.order.includes(tabId) ? unpinTab(state, tabId) : pinTab(state, tabId);
}

/**
 * Rettangolo di ogni pannello della vista. Con un pannello massimizzato quello
 * occupa tutta l'area e gli altri non compaiono nel risultato (nascosti).
 */
export function paneRects(group: Pick<ViewState, "order" | "maximizedTabId">): Record<string, Rect> {
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
  /** Id del gruppo che dispone il pannello: `GLOBAL_ID` nella vista globale. */
  groupWorkspaceId: string;
  /** Il pannello e' fissato nella vista globale. */
  pinned: boolean;
}

/**
 * Layout di tutti i pannelli, indicizzato per id. Ogni pannello ha un solo
 * rettangolo, preso dalla vista mostrata: con la vista globale i pannelli
 * fissati si dispongono nella griglia globale, tutti gli altri si nascondono.
 */
export function computePaneLayouts(state: LayoutState): Record<string, PaneLayout> {
  const out: Record<string, PaneLayout> = {};
  for (const g of state.groups) {
    const rects = paneRects(g);
    const groupActive =
      state.activeView === "workspace" && g.workspaceId === state.activeWorkspaceId;
    for (const id of g.order) {
      const rect = rects[id] ?? null;
      const visible = groupActive && rect !== null;
      out[id] = {
        rect,
        visible,
        focused: visible && g.activeTabId === id,
        maximized: g.maximizedTabId === id,
        groupWorkspaceId: g.workspaceId,
        pinned: state.global.order.includes(id),
      };
    }
  }
  if (state.activeView === "global") {
    for (const pl of Object.values(out)) pl.rect = null; // un solo rettangolo per tab
    const rects = paneRects(state.global);
    for (const id of state.global.order) {
      const rect = rects[id] ?? null;
      const visible = rect !== null;
      out[id] = {
        rect,
        visible,
        focused: visible && state.global.activeTabId === id,
        maximized: state.global.maximizedTabId === id,
        groupWorkspaceId: GLOBAL_ID,
        pinned: true,
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
