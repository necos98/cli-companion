// Viste del Tasks Manager: colonne del Kanban, regole di drag & drop, righe
// della Lista (filtri, conteggi, ordinamento, sottotitolo) e preferenze per
// workspace in localStorage (lo storage è iniettato). Funzioni pure a zero
// dipendenze (solo board.ts), importabili sia dall'app sia dai test.

import {
  STATUSES,
  doneTasks,
  filterTasks,
  formatRelative,
  formatTaskDuration,
  matchesQuery,
  taskDuration,
  updatedLabel,
  waitingOnLabel,
} from "./board.ts";
import type { Task, TaskFilter, TaskStatus } from "./board.ts";

// ---------------------------------------------------------------- viste

export type ManagerView = "kanban" | "list" | "graph";

export const MANAGER_VIEWS: { id: ManagerView; label: string }[] = [
  { id: "kanban", label: "Kanban" },
  { id: "list", label: "Lista" },
  { id: "graph", label: "Dipendenze" },
];

function isView(v: unknown): v is ManagerView {
  return v === "kanban" || v === "list" || v === "graph";
}

// ---------------------------------------------------------------- Kanban

export type KanbanColumnKey = TaskStatus;

export interface KanbanColumns {
  queued: Task[];
  running: Task[];
  /** Fatti degli ultimi 7 giorni, dal più recente. */
  done: Task[];
  failed: Task[];
  /** Fatti che corrispondono alla ricerca ma sono più vecchi di 7 giorni. */
  doneOlder: number;
}

/** Colonne del Kanban con la ricerca applicata; "Da fare" resta nell'ordine di partenza. */
export function kanbanColumns(tasks: Task[], query: string, now: number): KanbanColumns {
  const visible = tasks.filter((t) => matchesQuery(t, query));
  const done = doneTasks({ tasks: visible }, now);
  const doneAll = visible.filter((t) => t.status === "done").length;
  return {
    queued: visible.filter((t) => t.status === "queued"),
    running: visible.filter((t) => t.status === "running"),
    done,
    failed: visible.filter((t) => t.status === "failed").sort((a, b) => b.updated_at - a.updated_at),
    doneOlder: doneAll - done.length,
  };
}

/** Si trascinano i Da fare (riordino) e i Falliti (Riprova). */
export function isDraggable(status: TaskStatus): boolean {
  return status === "queued" || status === "failed";
}

/** La sola colonna "Da fare" accetta drop: da un Da fare (riordino) o da un Fallito (Riprova). */
export function acceptsDrop(dragged: TaskStatus, column: KanbanColumnKey): boolean {
  return column === "queued" && isDraggable(dragged);
}

/**
 * Nuova posizione di un Da fare trascinato sulla coda `queueIds` (ordine
 * completo): prima o dopo `overId` (null = in fondo). `beforeId` è
 * l'argomento di `task_move`; null se il drop non cambia l'ordine.
 */
export function reorderTarget(
  queueIds: string[],
  draggedId: string,
  overId: string | null,
  after: boolean,
): { beforeId: string | null } | null {
  if (!queueIds.includes(draggedId) || overId === draggedId) return null;
  const rest = queueIds.filter((id) => id !== draggedId);
  const at = overId === null ? -1 : rest.indexOf(overId);
  const idx = at === -1 ? rest.length : at + (after ? 1 : 0);
  const order = [...rest.slice(0, idx), draggedId, ...rest.slice(idx)];
  if (order.every((id, i) => id === queueIds[i])) return null;
  return { beforeId: order[idx + 1] ?? null };
}

/** Parti della riga di meta di una card: modello, tentativo, tempo o esito. */
export function kanbanMeta(task: Task, now: number): string[] {
  const parts: string[] = [];
  if (task.model) parts.push(task.model);
  switch (task.status) {
    case "queued":
      if (task.attempts > 0) parts.push(`tentativo ${task.attempts + 1}`);
      break;
    case "running": {
      const d = formatTaskDuration(task, now);
      if (d) parts.push(d);
      break;
    }
    case "done":
      parts.push(formatRelative(task.finished_at ?? task.updated_at, now));
      break;
    case "failed":
      if (task.attempts > 0) parts.push(`tentativo ${task.attempts}`);
      parts.push(updatedLabel(task, now));
      break;
  }
  return parts;
}

// ---------------------------------------------------------------- Lista

export type ListSortKey = "updated" | "title" | "duration";

export interface ListSort {
  key: ListSortKey;
  desc: boolean;
}

/** Default della Lista: aggiornati di recente per primi. */
export const DEFAULT_SORT: ListSort = { key: "updated", desc: true };

/** Clic sull'intestazione: stessa colonna = inverte, altra = direzione naturale (titolo A→Z, il resto decrescente). */
export function toggleSort(sort: ListSort, key: ListSortKey): ListSort {
  if (sort.key === key) return { key, desc: !sort.desc };
  return { key, desc: key !== "title" };
}

/** Ordina i task; i task senza durata vanno sempre in fondo. Non modifica `tasks`. */
export function sortTasks(tasks: Task[], sort: ListSort, now: number): Task[] {
  const dir = sort.desc ? -1 : 1;
  return tasks.slice().sort((a, b) => {
    switch (sort.key) {
      case "title":
        return dir * a.title.localeCompare(b.title, "it", { sensitivity: "base" });
      case "duration": {
        const da = taskDuration(a, now);
        const db = taskDuration(b, now);
        if (da === null || db === null) return da === db ? 0 : da === null ? 1 : -1;
        return dir * (da - db);
      }
      default:
        return dir * (a.updated_at - b.updated_at);
    }
  });
}

/** Righe della Lista: filtri applicati, poi ordinamento. */
export function listRows(tasks: Task[], filter: TaskFilter, sort: ListSort, now: number): Task[] {
  return sortTasks(filterTasks(tasks, filter), sort, now);
}

/** Conteggi dei chip di stato: rispecchiano ricerca e repository, non la selezione degli stati. */
export function statusCounts(tasks: Task[], filter: Pick<TaskFilter, "query" | "repos">): Record<TaskStatus, number> {
  const counts = { queued: 0, running: 0, done: 0, failed: 0 };
  for (const t of filterTasks(tasks, { ...filter, statuses: [] })) counts[t.status]++;
  return counts;
}

/** Posizione (da 1) di ogni Da fare nella coda. */
export function queuePositions(tasks: Pick<Task, "id" | "status">[]): Map<string, number> {
  const map = new Map<string, number>();
  for (const t of tasks) if (t.status === "queued") map.set(t.id, map.size + 1);
  return map;
}

/** Sottotitolo di una riga: l'esito, il motivo del fallimento o a che punto è. */
export function listSubtitle(task: Task, tasks: Pick<Task, "id" | "title">[], positions: Map<string, number>): string {
  switch (task.status) {
    case "done":
      return task.report?.summary || task.last_report || "";
    case "failed":
      return task.note || task.report?.summary || task.last_report || "";
    case "running":
      return task.preparing || task.last_report || "";
    default: {
      const waiting = waitingOnLabel(task, tasks);
      if (waiting) return waiting;
      const pos = positions.get(task.id);
      return pos === 1 ? "Prossimo in coda" : pos ? `In coda, posizione ${pos}` : "";
    }
  }
}

// ---------------------------------------------------------------- preferenze

/** Chiave di localStorage: oggetto JSON `{ [workspaceId]: WorkspacePrefs }`. */
export const MANAGER_PREFS_KEY = "cli-companion.tasks-manager.prefs";

export interface ListPrefs {
  /** Stati ammessi; vuoto = tutti. */
  statuses: TaskStatus[];
  /** Repository ammessi; vuoto = tutti. */
  repos: string[];
  sort: ListSort;
}

export interface WorkspacePrefs {
  view: ManagerView;
  list: ListPrefs;
}

export function defaultPrefs(): WorkspacePrefs {
  return { view: "kanban", list: { statuses: [], repos: [], sort: { ...DEFAULT_SORT } } };
}

/** Parte minima di `Storage` che serve qui. */
export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function readAll(storage: StorageLike | null | undefined): Record<string, unknown> {
  try {
    const raw = storage?.getItem(MANAGER_PREFS_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    return typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : {};
  } catch {
    return {};
  }
}

/** Preferenze validate del workspace; storage assente o rotto = default. */
export function readPrefs(storage: StorageLike | null | undefined, workspaceId: string): WorkspacePrefs {
  const out = defaultPrefs();
  const raw = readAll(storage)[workspaceId];
  if (typeof raw !== "object" || raw === null) return out;
  const p = raw as Record<string, unknown>;
  if (isView(p.view)) out.view = p.view;
  const list = p.list;
  if (typeof list === "object" && list !== null) {
    const l = list as Record<string, unknown>;
    if (Array.isArray(l.statuses)) out.list.statuses = STATUSES.filter((s) => (l.statuses as unknown[]).includes(s));
    if (Array.isArray(l.repos)) out.list.repos = l.repos.filter((r): r is string => typeof r === "string");
    const sort = l.sort as Record<string, unknown> | null | undefined;
    if (sort && (sort.key === "updated" || sort.key === "title" || sort.key === "duration")) {
      out.list.sort = { key: sort.key, desc: sort.desc === true };
    }
  }
  return out;
}

/** Salva le preferenze del workspace; gli errori di storage sono ignorati. */
export function writePrefs(
  storage: StorageLike | null | undefined,
  workspaceId: string,
  prefs: WorkspacePrefs,
): void {
  try {
    storage?.setItem(MANAGER_PREFS_KEY, JSON.stringify({ ...readAll(storage), [workspaceId]: prefs }));
  } catch {
    // Storage non disponibile (finestra privata, quota): le preferenze valgono solo per questa apertura.
  }
}

// ---------------------------------------------------------------- tastiera

/** Il bersaglio di un evento è un campo di testo (anche l'input nascosto di xterm)? */
export function isTypingTarget(target: { tagName?: string; isContentEditable?: boolean } | null | undefined): boolean {
  const tag = target?.tagName?.toUpperCase();
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || target?.isContentEditable === true;
}
