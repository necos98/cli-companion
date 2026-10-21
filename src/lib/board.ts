// Board dei task di un workspace: tipi del contratto (docs/BOARD.MD), etichette,
// colori, azioni consentite e conteggi. Funzioni pure a zero dipendenze (solo
// automation.ts), importabili sia dall'app sia dai test `node --test`.

import { statusWord, truncateReport } from "./automation.ts";
import type { BoardAutomation } from "./schedule.ts";

export type TaskStatus =
  | "queued"
  | "running"
  | "review"
  | "done"
  | "failed"
  | "blocked"
  | "cancelled";

/** Colonna derivata dal backend (campo `column` di ogni task). */
export type TaskColumn =
  | "backlog"
  | "ready"
  | "running"
  | "review"
  | "done"
  | "failed"
  | "blocked"
  | "cancelled";

/** Azioni dell'utente sul task (`board_task_action`). */
export type TaskAction = "hold" | "unhold" | "retry" | "cancel" | "done";

export interface TaskWorktree {
  path: string;
  branch: string;
  base: string;
  /** Repository del task (cartella relativa alla root); assente = la root. */
  repo?: string | null;
  /** Setup del repository completato; false = da rifare al prossimo avvio. */
  setup_done?: boolean;
}

/** Branch di integrazione di un repository (chiave "." = root nella mappa). */
export interface BoardIntegration {
  branch: string;
  path: string;
  base: string;
}

export interface TaskTestResult {
  command: string;
  result: "passed" | "failed" | "not_run" | string;
  details?: string | null;
}

export interface TaskReport {
  status: "completed" | "failed" | "blocked" | string;
  summary: string;
  commit?: string | null;
  files_changed?: string[];
  tests?: TaskTestResult[];
  follow_ups?: string[];
  diffstat?: string | null;
}

export interface Task {
  id: string;
  title: string;
  task: string;
  depends_on: string[];
  touches: string[];
  isolation: "worktree" | "none" | string;
  base_branch?: string | null;
  /** Repository (cartella relativa alla root del workspace); assente = la root. */
  repo?: string | null;
  agent: string;
  model?: string | null;
  permission_mode?: string | null;
  priority: number;
  held: boolean;
  status: TaskStatus;
  column: TaskColumn;
  worker_status?: string | null;
  /** Passo di preparazione in corso (setup del repository): l'app sta preparando il worktree. */
  preparing?: string | null;
  session?: string | null;
  attempts: number;
  worktree?: TaskWorktree | null;
  last_report?: string | null;
  note?: string | null;
  /** Completato fuori dalla board (done da queued, con nota): niente worker né worktree. */
  external?: boolean;
  /** Nota privata dell'orchestratore: mai inviata al worker. */
  orchestrator_note?: string | null;
  report?: TaskReport | null;
  created_at: number;
  updated_at: number;
  started_at?: number | null;
  finished_at?: number | null;
}

export interface BoardView {
  workspace_id: string;
  max_parallel: number;
  paused: boolean;
  base_branch?: string | null;
  /** Branch base per repository ("." = root). */
  base_branches?: Record<string, string>;
  /** Modello del branch di integrazione ({date} = data locale). */
  integration_branch?: string | null;
  /** Integrazioni risolte per repository ("." = root). */
  integrations?: Record<string, BoardIntegration>;
  /** Repository rilevati nel workspace; null finché non calcolati. */
  repos?: string[] | null;
  owner?: string | null;
  updated_at: number;
  /** Revisione monotona: cresce a ogni modifica della board serializzata. */
  rev?: number;
  tasks: Task[];
  /** Presente solo per i workspace automatici (docs/SCHEDULE.MD). */
  automation?: BoardAutomation | null;
}

export const MAX_PARALLEL_MIN = 1;
export const MAX_PARALLEL_MAX = 16;

/** Ordine di legenda e di conteggio delle colonne. */
export const COLUMNS: TaskColumn[] = [
  "backlog",
  "ready",
  "running",
  "review",
  "done",
  "failed",
  "blocked",
  "cancelled",
];

const COLUMN_LABELS: Record<TaskColumn, string> = {
  backlog: "In attesa",
  ready: "Pronto",
  running: "In corso",
  review: "Da integrare",
  done: "Fatto",
  failed: "Fallito",
  blocked: "Bloccato",
  cancelled: "Annullato",
};

/** Colore (token del tema) del testo/punto di ogni colonna. */
const COLUMN_TEXT: Record<TaskColumn, string> = {
  backlog: "text-muted-foreground",
  ready: "text-primary",
  running: "text-status-running",
  review: "text-status-attention",
  done: "text-status-finished",
  failed: "text-destructive",
  blocked: "text-status-attention",
  cancelled: "text-faint",
};

/** Colore di sfondo del punto/badge (stesso colore del testo). */
const COLUMN_DOT: Record<TaskColumn, string> = {
  backlog: "bg-muted-foreground",
  ready: "bg-primary",
  running: "bg-status-running",
  review: "bg-status-attention",
  done: "bg-status-finished",
  failed: "bg-destructive",
  blocked: "bg-status-attention",
  cancelled: "bg-faint",
};

/** Bordo del nodo nel grafo. */
const COLUMN_BORDER: Record<TaskColumn, string> = {
  backlog: "border-border-strong",
  ready: "border-primary/60",
  running: "border-status-running/70",
  review: "border-status-attention/70",
  done: "border-status-finished/50",
  failed: "border-destructive/70",
  blocked: "border-status-attention/50",
  cancelled: "border-border",
};

/** Colore esadecimale per minimappa/archi (stessi valori di index.css). */
const COLUMN_HEX: Record<TaskColumn, string> = {
  backlog: "#8b99a8",
  ready: "#60a5fa",
  running: "#f5b544",
  review: "#fb923c",
  done: "#34d399",
  failed: "#ef4444",
  blocked: "#fb923c",
  cancelled: "#5d6b79",
};

/** Testo del marcatore dei task completati fuori dalla board. */
export const EXTERNAL_LABEL = "Completato fuori board";

/** Task trattenuti (in coda, non partiranno finché non vengono rilasciati). */
export function heldCount(tasks: Pick<Task, "held" | "status">[]): number {
  let n = 0;
  for (const t of tasks) if (t.held && t.status === "queued") n++;
  return n;
}

export function columnLabel(c: string): string {
  return COLUMN_LABELS[c as TaskColumn] ?? c;
}

export function columnTextClass(c: string): string {
  return COLUMN_TEXT[c as TaskColumn] ?? "text-muted-foreground";
}

export function columnDotClass(c: string): string {
  return COLUMN_DOT[c as TaskColumn] ?? "bg-muted-foreground";
}

export function columnBorderClass(c: string): string {
  return COLUMN_BORDER[c as TaskColumn] ?? "border-border-strong";
}

export function columnHex(c: string): string {
  return COLUMN_HEX[c as TaskColumn] ?? "#8b99a8";
}

/** Etichette dei pulsanti d'azione. */
export const ACTION_LABELS: Record<TaskAction, string> = {
  hold: "Trattieni",
  unhold: "Rilascia",
  retry: "Riprova",
  cancel: "Annulla",
  done: "Segna come fatto",
};

/**
 * Azioni consentite per un task (regole del ciclo di vita):
 * hold/unhold solo per `queued`; retry da failed/blocked/cancelled/review;
 * cancel da qualunque stato non `done` né `cancelled`; done da review/failed/blocked
 * (da `queued` il backend lo accetta solo con una nota: la UI non lo offre, lo fa l'orchestratore).
 */
/** Il modello di un task si cambia solo prima che parta (o dopo che è finito male). */
export function canEditModel(status: TaskStatus): boolean {
  return (
    status === "queued" || status === "failed" || status === "blocked" || status === "cancelled"
  );
}

export function allowedActions(
  task: Pick<Task, "status" | "held">,
): TaskAction[] {
  const out: TaskAction[] = [];
  if (task.status === "queued") out.push(task.held ? "unhold" : "hold");
  if (
    task.status === "failed" ||
    task.status === "blocked" ||
    task.status === "cancelled" ||
    task.status === "review"
  ) {
    out.push("retry");
  }
  if (task.status === "review" || task.status === "failed" || task.status === "blocked") {
    out.push("done");
  }
  if (task.status !== "done" && task.status !== "cancelled") out.push("cancel");
  return out;
}

/** "Apri terminale" ha senso solo se il task ha un tab worker. */
export function canOpenTerminal(task: Pick<Task, "session">): boolean {
  return typeof task.session === "string" && task.session !== "";
}

/** Task conclusi (fatti) sul totale. Gli annullati non contano come fatti. */
export function progress(tasks: Pick<Task, "status">[]): { done: number; total: number } {
  let done = 0;
  for (const t of tasks) if (t.status === "done") done++;
  return { done, total: tasks.length };
}

/** Numero di task per colonna (tutte le colonne presenti, anche a zero). */
export function columnCounts(tasks: Pick<Task, "column">[]): Record<TaskColumn, number> {
  const counts = Object.fromEntries(COLUMNS.map((c) => [c, 0])) as Record<TaskColumn, number>;
  for (const t of tasks) if (t.column in counts) counts[t.column]++;
  return counts;
}

/** Chiave dell'insieme degli id dei task (per riadattare la vista solo quando cambia). */
export function taskIdsKey(tasks: Pick<Task, "id">[]): string {
  return tasks
    .map((t) => t.id)
    .sort()
    .join("\u0000");
}

/** Task attivi: in corso o da integrare (numero sul pulsante Board della sidebar). */
export function activeCount(board: Pick<BoardView, "tasks"> | null | undefined): number {
  if (!board) return 0;
  let n = 0;
  for (const t of board.tasks) if (t.status === "running" || t.status === "review") n++;
  return n;
}

/** Etichetta del task mentre l'app ne prepara il worktree. */
export const PREPARING_LABEL = "Preparazione…";

/** Sotto-stato del worker di un task in corso (parola italiana + colore), se c'e'. */
export function workerSubStatus(
  task: Pick<Task, "status" | "worker_status" | "preparing">,
): { text: string; className: string } | null {
  // Il worker non c'e' ancora: l'app esegue il setup del repository.
  if (task.status === "running" && task.preparing) {
    return { text: PREPARING_LABEL, className: "text-status-attention" };
  }
  if (task.status !== "running" || !task.worker_status) return null;
  return statusWord(task.worker_status);
}

/** Limita `max_parallel` all'intervallo consentito. */
export function clampMaxParallel(n: number): number {
  if (!Number.isFinite(n)) return MAX_PARALLEL_MIN;
  return Math.min(MAX_PARALLEL_MAX, Math.max(MAX_PARALLEL_MIN, Math.round(n)));
}

/** Ultimo report accorciato per il nodo. */
export function shortReport(task: Pick<Task, "last_report">, max = 90): string {
  return task.last_report ? truncateReport(task.last_report, max) : "";
}

/** Stesso id di workspace? (evento `board-update` di un'altra board: ignorato) */
export function isBoardOf(board: Pick<BoardView, "workspace_id">, workspaceId: string): boolean {
  return board.workspace_id === workspaceId;
}

/** Board vuota segnaposto (prima del caricamento o con il backend assente). */
export function emptyBoard(workspaceId: string): BoardView {
  return {
    workspace_id: workspaceId,
    max_parallel: 3,
    paused: false,
    base_branch: null,
    owner: null,
    updated_at: 0,
    tasks: [],
  };
}

/** Testo dello stato vuoto. */
export const EMPTY_BOARD_TEXT =
  "Nessun piano: chiedi a un orchestratore (skill orchestrate) di crearne uno";

/**
 * Sceglie fra la board già nota e una appena arrivata (risposta di un comando
 * o evento): una risposta più vecchia di quella nota non la sostituisce.
 */
export function mergeBoard(current: BoardView | undefined, next: BoardView): BoardView {
  if (!current) return next;
  if (typeof current.rev === "number" && typeof next.rev === "number") {
    // rev piu' bassa = dato vecchio; rev uguale = stessi dati, si tiene quello noto.
    return next.rev > current.rev ? next : current;
  }
  // Senza `rev` (backend vecchio): ripiego su updated_at.
  return current.updated_at > next.updated_at ? current : next;
}

/** Lunghezza massima delle regole per i worker del workspace. */
export const WORKER_RULES_MAX = 4000;

/** Regole normalizzate: trim e massimo `WORKER_RULES_MAX` caratteri. */
export function sanitizeWorkerRules(raw: string): string {
  return raw.trim().slice(0, WORKER_RULES_MAX).trim();
}

/** Lunghezza massima del comando di setup dei worktree. */
export const WORKTREE_SETUP_MAX = 1000;

/** Comando di setup normalizzato: trim e massimo `WORKTREE_SETUP_MAX` caratteri ("" = nessuno). */
export function sanitizeWorktreeSetup(raw: string): string {
  return raw.trim().slice(0, WORKTREE_SETUP_MAX).trim();
}
