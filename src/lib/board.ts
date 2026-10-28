// Coda dei task di un workspace ("Tasks Manager"): tipi del contratto
// (docs/BOARD.MD), stati con etichette e colori, selettori, filtri, formati di
// durata e consegna. Funzioni pure a zero dipendenze (solo automation.ts),
// importabili sia dall'app sia dai test `node --test`.

import { statusWord, truncateReport } from "./automation.ts";
import type { WorkerPermission } from "./workerPermission.ts";

// ---------------------------------------------------------------- tipi

/** Stati di un task: i soli quattro del contratto. */
export type TaskStatus = "queued" | "running" | "done" | "failed";

/** Tipo di task: `work` (default) o `review` (verifica del lavoro dei task da cui dipende). */
export type TaskKind = "work" | "review";

/** Verdetto di una review completata. */
export type ReviewVerdict = "passed" | "issues";

/** Stato mostrato: i quattro stati più "Richiede attenzione" (running con `attention`). */
export type DisplayStatus = TaskStatus | "attention";

/** Azioni dell'utente sul task (`task_action`). */
export type TaskAction = "retry" | "mark_done" | "stop" | "delete" | "to_top";

export type DeliveryMode = "pr" | "local_merge" | "push" | "commit";
export type MergeMethod = "squash" | "merge" | "rebase";

/** Consegna del workspace (campo `delivery`, serde snake_case). */
export interface Delivery {
  mode: DeliveryMode;
  /** Un worktree per task: ha senso solo per `pr` e `local_merge`. */
  parallel: boolean;
  /** Solo per `pr`. */
  merge_method: MergeMethod;
}

export interface TaskWorktree {
  path: string;
  branch: string;
  base: string;
  /** Setup del repository completato; false = da rifare al prossimo avvio. */
  setup_done?: boolean;
}

export interface TaskTestResult {
  command: string;
  result: "passed" | "failed" | "not_run" | string;
  details?: string | null;
}

export interface TaskReport {
  status: "completed" | "failed" | string;
  summary: string;
  commit?: string | null;
  pr_url?: string | null;
  files_changed?: string[];
  tests?: TaskTestResult[];
  follow_ups?: string[];
  diffstat?: string | null;
  /** Solo per i task di review completati: esito della verifica. */
  verdict?: ReviewVerdict | null;
  /** Solo per le review: problemi (con `issues`) o suggerimenti (con `passed`). */
  findings?: string[];
}

/** Esito del tentativo precedente, dopo "Riprova". */
export interface PreviousAttempt {
  summary?: string | null;
  note?: string | null;
}

/** Task come lo espone `BoardView` (campi del task + campi derivati). */
export interface Task {
  id: string;
  title: string;
  /** Specifica per il worker (markdown). */
  task: string;
  depends_on: string[];
  /** Tipo di task; assente = `work`. Una `review` verifica i task da cui dipende. */
  kind?: TaskKind;
  /** Repository (cartella relativa alla root del workspace); null/assente = la root. */
  repo?: string | null;
  agent: string;
  /** null/assente = modello dei worker del workspace. */
  model?: string | null;
  status: TaskStatus;
  worker_status?: string | null;
  /** Tab del worker corrente (chiave del tab nella griglia dei terminali). */
  session?: string | null;
  attempts: number;
  worktree?: TaskWorktree | null;
  last_report?: string | null;
  note?: string | null;
  report?: TaskReport | null;
  created_at: number;
  updated_at: number;
  started_at?: number | null;
  finished_at?: number | null;
  /** Passo di preparazione in corso (setup del repository): il worker non c'è ancora. */
  preparing?: string | null;
  previous_attempt?: PreviousAttempt | null;
  /** Dipendenze non ancora `done` (solo per i task `queued`). */
  waiting_on: string[];
  /** Il worker aspetta l'utente (`needs_input`/`idle`): badge "Richiede attenzione". */
  attention: boolean;
}

/** Specifica di un task in una proposta. */
export interface TaskSpec {
  id: string;
  title: string;
  task: string;
  depends_on: string[];
  repo?: string | null;
  model?: string | null;
  /** Tipo di task; assente = `work`. Una `review` richiede almeno una dipendenza. */
  kind?: TaskKind;
}

/** Proposta di Task Intake in attesa di Accetta / Scarta. */
export interface Proposal {
  id: string;
  /** Tab dell'Intake (o del reviewer) che l'ha proposta. */
  source_tab: string;
  /** Task di review il cui reviewer l'ha proposta; assente/null = proposta dell'Intake. */
  source_task?: string | null;
  created_at: number;
  tasks: TaskSpec[];
}

/** Patch di un repository: assente = invariato, `null` dove indicato = azzera. */
export interface RepoPatch {
  base_branch?: string | null;
  container_root?: string | null;
  copy_ignored?: string[];
  setup?: string[];
  cleanup?: string[];
  remove_force?: boolean;
}

/** Modifiche alle impostazioni del workspace: ogni campo assente = invariato. */
export interface SettingsPatch {
  worker_rules?: string;
  worker_model?: string | null;
  worker_permission?: WorkerPermission;
  worker_allow?: string[];
  worker_deny?: string[];
  delivery?: Partial<Delivery>;
  use_schedule?: boolean;
  default_launch?: string | null;
  launches?: { id?: string; name: string; command: string }[];
  /** Per repository (chiave = cartella); `null` = rimuove la configurazione. */
  repos?: Record<string, RepoPatch | null>;
}

/** Proposta di modifica alle impostazioni in attesa di Applica / Scarta. */
export interface SettingsProposalView {
  id: string;
  /** Tab che l'ha proposta. */
  source_tab: string;
  created_at: number;
  reason: string;
  patch: SettingsPatch;
  /** Valori attuali dei soli campi toccati dal patch. */
  current: SettingsPatch;
  /** Campi toccati che eseguono comandi o allargano i permessi (`repos[<key>].setup`, ...). */
  sensitive: string[];
}

export interface BoardView {
  workspace_id: string;
  /** Revisione monotona: cresce a ogni modifica della vista. */
  rev: number;
  /** Worker in parallelo (vale solo in modalità parallela). */
  max_parallel: number;
  paused: boolean;
  updated_at: number;
  /** L'ordine dell'array È l'ordine della coda. */
  tasks: Task[];
  proposals: Proposal[];
  settings_proposals: SettingsProposalView[];
  /** Il workspace usa le fasce orarie e adesso la fascia è chiusa. */
  schedule_blocked: boolean;
  delivery: Delivery;
}

/** Argomento di `task_create`: l'id lo genera il backend dal titolo. */
export interface NewTaskSpec {
  title: string;
  task: string;
  depends_on?: string[];
  repo?: string | null;
  model?: string | null;
  /** Tipo di task; assente = `work`. Una `review` richiede almeno una dipendenza. */
  kind?: TaskKind;
}

/** Argomento di `task_update`: i campi assenti non cambiano, `null` azzera repo/model. */
export interface TaskPatch {
  title?: string;
  task?: string;
  depends_on?: string[];
  repo?: string | null;
  model?: string | null;
  /** Tipo di task; assente = invariato (e nei file vecchi `work`). */
  kind?: TaskKind;
}

/** Evento `task-notify`: notifica di sistema per un task. */
export interface TaskNotifyEvent {
  workspace_id: string;
  task_id: string;
  tab_id: string | null;
  kind: "attention" | "failed";
  title: string;
  body: string;
}

// ---------------------------------------------------------------- stati

export const MAX_PARALLEL_MIN = 1;
export const MAX_PARALLEL_MAX = 16;

/** Ordine dei quattro stati (legenda, filtri, conteggi). */
export const STATUSES: TaskStatus[] = ["queued", "running", "done", "failed"];

interface StatusMeta {
  label: string;
  /** Colore del testo. */
  text: string;
  /** Colore del punto/badge. */
  dot: string;
  /** Bordo del nodo nel grafo. */
  border: string;
  /** Colore esadecimale per minimappa/archi (stessi valori di index.css). */
  hex: string;
}

/**
 * Da fare = grigio, In corso = ambra, Richiede attenzione = arancione, Fatto =
 * verde, Fallito = rosso. Il blu (primary) è solo per azioni e proposte.
 */
const META: Record<DisplayStatus, StatusMeta> = {
  queued: {
    label: "Da fare",
    text: "text-muted-foreground",
    dot: "bg-muted-foreground",
    border: "border-border-strong",
    hex: "#8a99a8",
  },
  running: {
    label: "In corso",
    text: "text-status-running",
    dot: "bg-status-running",
    border: "border-status-running/70",
    hex: "#f5b544",
  },
  attention: {
    label: "Richiede attenzione",
    text: "text-status-attention",
    dot: "bg-status-attention",
    border: "border-status-attention/70",
    hex: "#fb923c",
  },
  done: {
    label: "Fatto",
    text: "text-status-finished",
    dot: "bg-status-finished",
    border: "border-status-finished/50",
    hex: "#34d399",
  },
  failed: {
    label: "Fallito",
    text: "text-destructive",
    dot: "bg-destructive",
    border: "border-destructive/70",
    hex: "#f87171",
  },
};

const FALLBACK = META.queued;

function metaOf(s: string): StatusMeta {
  return META[s as DisplayStatus] ?? FALLBACK;
}

/** Stato da mostrare: un task in corso che aspetta l'utente è "attention". */
export function displayStatus(task: Pick<Task, "status" | "attention">): DisplayStatus {
  return task.status === "running" && task.attention ? "attention" : task.status;
}

export function statusLabel(s: string): string {
  return META[s as DisplayStatus]?.label ?? s;
}
export function statusTextClass(s: string): string {
  return metaOf(s).text;
}
export function statusDotClass(s: string): string {
  return metaOf(s).dot;
}
export function statusBorderClass(s: string): string {
  return metaOf(s).border;
}
export function statusHex(s: string): string {
  return metaOf(s).hex;
}

// ---------------------------------------------------------------- selettori

type Tasks = Pick<BoardView, "tasks">;

/** Task Da fare, nell'ordine di partenza (quello dell'array). */
export function queuedTasks(board: Tasks): Task[] {
  return board.tasks.filter((t) => t.status === "queued");
}

/** Task in corso, nell'ordine dell'array. */
export function runningTasks(board: Tasks): Task[] {
  return board.tasks.filter((t) => t.status === "running");
}

export function failedTasks(board: Tasks): Task[] {
  return board.tasks.filter((t) => t.status === "failed");
}

/** Giorni di storico dei Fatti mostrati di default (Kanban e pannello). */
export const DONE_RECENT_DAYS = 7;

const DAY_MS = 86_400_000;

/** Istante di fine (o ultimo aggiornamento) di un task concluso. */
function finishedAt(t: Pick<Task, "finished_at" | "updated_at">): number {
  return t.finished_at ?? t.updated_at;
}

/** Task Fatti, dal più recente; con `days` solo quelli degli ultimi N giorni. */
export function doneTasks(board: Tasks, now: number, days: number | null = DONE_RECENT_DAYS): Task[] {
  const limit = days === null ? -Infinity : now - days * DAY_MS;
  return board.tasks
    .filter((t) => t.status === "done" && finishedAt(t) >= limit)
    .sort((a, b) => finishedAt(b) - finishedAt(a));
}

export interface TaskCounts {
  proposals: number;
  queued: number;
  running: number;
  done: number;
  failed: number;
  /** In corso che richiedono attenzione (sottoinsieme di `running`). */
  attention: number;
  total: number;
}

/** Proposte in attesa di una decisione: di task e di impostazioni. */
export function pendingProposals(
  board: Partial<Pick<BoardView, "proposals" | "settings_proposals">> | null | undefined,
): number {
  return (board?.proposals?.length ?? 0) + (board?.settings_proposals?.length ?? 0);
}

/** Conteggi della board; `null` = tutto a zero. */
export function taskCounts(
  board: Partial<Pick<BoardView, "tasks" | "proposals" | "settings_proposals">> | null | undefined,
): TaskCounts {
  const c: TaskCounts = {
    proposals: pendingProposals(board),
    queued: 0,
    running: 0,
    done: 0,
    failed: 0,
    attention: 0,
    total: 0,
  };
  for (const t of board?.tasks ?? []) {
    c.total++;
    if (t.status in c) c[t.status]++;
    if (t.status === "running" && t.attention) c.attention++;
  }
  return c;
}

/** In corso che aspettano l'utente. */
export function attentionCount(board: Tasks | null | undefined): number {
  return taskCounts(board).attention;
}

/** Task attivi (in corso) per il contatore della sidebar. */
export function activeCount(board: Tasks | null | undefined): number {
  return taskCounts(board).running;
}

/** Task Fatti sul totale. */
export function progress(tasks: Pick<Task, "status">[]): { done: number; total: number } {
  let done = 0;
  for (const t of tasks) if (t.status === "done") done++;
  return { done, total: tasks.length };
}

/** Task da cui può dipendere un nuovo task: tutti quelli non ancora Fatti. */
export function dependencyCandidates(board: Tasks): Task[] {
  return board.tasks.filter((t) => t.status !== "done");
}

/** Repository distinti dei task (la root come "."), ordinati; serve ai filtri dei multi-repo. */
export function taskRepos(tasks: Pick<Task, "repo">[]): string[] {
  return [...new Set(tasks.map((t) => t.repo || "."))].sort();
}

/** Chiave dell'insieme degli id dei task (per riadattare la vista solo quando cambia). */
export function taskIdsKey(tasks: Pick<Task, "id">[]): string {
  return tasks
    .map((t) => t.id)
    .sort()
    .join("\u0000");
}

/** Limita `max_parallel` all'intervallo consentito. */
export function clampMaxParallel(n: number): number {
  if (!Number.isFinite(n)) return MAX_PARALLEL_MIN;
  return Math.min(MAX_PARALLEL_MAX, Math.max(MAX_PARALLEL_MIN, Math.round(n)));
}

// ---------------------------------------------------------------- azioni

export const ACTION_LABELS: Record<TaskAction, string> = {
  retry: "Riprova",
  mark_done: "Segna come fatto",
  stop: "Ferma",
  delete: "Elimina",
  to_top: "In cima alla coda",
};

/**
 * Azioni consentite per stato: Da fare = in cima alla coda ed elimina; In corso
 * = ferma; Fallito = riprova, segna come fatto ed elimina; Fatto = nessuna.
 */
export function allowedActions(task: Pick<Task, "status">): TaskAction[] {
  switch (task.status) {
    case "queued":
      return ["to_top", "delete"];
    case "running":
      return ["stop"];
    case "failed":
      return ["retry", "mark_done", "delete"];
    default:
      return [];
  }
}

/** Titolo, testo, dipendenze, modello e repo si cambiano solo prima che parta o dopo un fallimento. */
export function canEditTask(task: Pick<Task, "status">): boolean {
  return task.status === "queued" || task.status === "failed";
}

/** Si elimina un Da fare / Fallito che nessun task non Fatto elenca in `depends_on`. */
export function canDeleteTask(
  task: Pick<Task, "id" | "status">,
  board: { tasks: Pick<Task, "id" | "status" | "depends_on">[] },
): boolean {
  if (task.status !== "queued" && task.status !== "failed") return false;
  return !board.tasks.some(
    (t) => t.id !== task.id && t.status !== "done" && (t.depends_on ?? []).includes(task.id),
  );
}

/** Id dei task non Fatti che dipendono da `taskId` (il motivo per cui non si elimina). */
export function dependentsOf(
  taskId: string,
  board: { tasks: Pick<Task, "id" | "status" | "depends_on">[] },
): string[] {
  return board.tasks
    .filter((t) => t.id !== taskId && t.status !== "done" && (t.depends_on ?? []).includes(taskId))
    .map((t) => t.id);
}

/** "Apri terminale" ha senso solo se il task ha un tab worker. */
export function canOpenTerminal(task: Pick<Task, "session">): boolean {
  return typeof task.session === "string" && task.session !== "";
}

/** Link della PR, se la consegna ne ha creata una. */
export function prUrl(task: Pick<Task, "report">): string | null {
  const u = task.report?.pr_url;
  return typeof u === "string" && /^https?:\/\//.test(u) ? u : null;
}

// ---------------------------------------------------------------- testi e formati

/** "attende X" (un solo task: il suo titolo) o "attende N task"; null se non aspetta. */
export function waitingOnLabel(
  task: Pick<Task, "waiting_on" | "status">,
  tasks: Pick<Task, "id" | "title">[],
): string | null {
  const w = task.waiting_on ?? [];
  if (task.status !== "queued" || w.length === 0) return null;
  if (w.length === 1) {
    const title = tasks.find((t) => t.id === w[0])?.title;
    return `attende ${title ?? w[0]}`;
  }
  return `attende ${w.length} task`;
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

/** Ultimo report accorciato per le card. */
export function shortReport(task: Pick<Task, "last_report">, max = 90): string {
  return task.last_report ? truncateReport(task.last_report, max) : "";
}

/** Durata compatta: "45s", "12m", "1h 05m", "2g 3h". */
export function formatDuration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ${String(m % 60).padStart(2, "0")}m`;
  return `${Math.floor(h / 24)}g ${h % 24}h`;
}

/**
 * Tempo di lavoro di un task: dall'avvio a ora se è in corso, dall'avvio alla
 * fine se concluso; null se non è mai partito.
 */
export function taskDuration(
  task: Pick<Task, "status" | "started_at" | "finished_at" | "updated_at">,
  now: number,
): number | null {
  if (!task.started_at) return null;
  if (task.status === "running") return Math.max(0, now - task.started_at);
  if (task.status === "queued") return null;
  return Math.max(0, finishedAt(task) - task.started_at);
}

/** Durata di un task formattata ("" se non applicabile). */
export function formatTaskDuration(task: Parameters<typeof taskDuration>[0], now: number): string {
  const d = taskDuration(task, now);
  return d === null ? "" : formatDuration(d);
}

/** "adesso", "20 min fa", "2 h fa", "ieri", "3 g fa", poi la data (gg/mm/aaaa). */
export function formatRelative(ms: number, now: number): string {
  const diff = now - ms;
  if (diff < 45_000) return "adesso";
  const min = Math.round(diff / 60_000);
  if (min < 60) return `${Math.max(1, min)} min fa`;
  const h = Math.round(diff / 3_600_000);
  if (h < 24) return `${h} h fa`;
  const days = Math.round(diff / DAY_MS);
  if (days === 1) return "ieri";
  if (days < 7) return `${days} g fa`;
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getDate())}/${pad(d.getMonth() + 1)}/${d.getFullYear()}`;
}

/** "aggiornato 20 min fa". */
export function updatedLabel(task: Pick<Task, "updated_at">, now: number): string {
  return `aggiornato ${formatRelative(task.updated_at, now)}`;
}

// ---------------------------------------------------------------- ricerca e filtri

/** Il task contiene `query` (titolo, specifica, esito, nota, report breve, id)? Maiuscole ignorate. */
export function matchesQuery(task: Task, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (q === "") return true;
  const haystack = [
    task.title,
    task.task,
    task.report?.summary,
    task.note,
    task.last_report,
    task.id,
  ];
  return haystack.some((h) => typeof h === "string" && h.toLowerCase().includes(q));
}

export interface TaskFilter {
  query: string;
  /** Stati ammessi; vuoto = tutti. */
  statuses: TaskStatus[];
  /** Repository ammessi ("." = la root); vuoto = tutti. */
  repos: string[];
}

export const EMPTY_FILTER: TaskFilter = { query: "", statuses: [], repos: [] };

/** C'è almeno un filtro attivo? (per "Azzera filtri") */
export function isFiltering(f: TaskFilter): boolean {
  return f.query.trim() !== "" || f.statuses.length > 0 || f.repos.length > 0;
}

/** Applica ricerca, stati e repository (l'ordine dei task non cambia). */
export function filterTasks(tasks: Task[], f: TaskFilter): Task[] {
  return tasks.filter(
    (t) =>
      (f.statuses.length === 0 || f.statuses.includes(t.status)) &&
      (f.repos.length === 0 || f.repos.includes(t.repo || ".")) &&
      matchesQuery(t, f.query),
  );
}

/** Aggiunge o toglie `status` dalla selezione multipla dei chip. */
export function toggleStatus(statuses: TaskStatus[], status: TaskStatus): TaskStatus[] {
  return statuses.includes(status) ? statuses.filter((s) => s !== status) : [...statuses, status];
}

/** Ordinamento della Lista: più recentemente aggiornati prima (discendente di default). */
export function sortByUpdated(tasks: Task[], desc = true): Task[] {
  return tasks.slice().sort((a, b) => (desc ? b.updated_at - a.updated_at : a.updated_at - b.updated_at));
}

// ---------------------------------------------------------------- consegna

export const DELIVERY_MODES: DeliveryMode[] = ["pr", "local_merge", "push", "commit"];
export const MERGE_METHODS: MergeMethod[] = ["squash", "merge", "rebase"];

export const DEFAULT_DELIVERY: Delivery = { mode: "commit", parallel: false, merge_method: "squash" };

const DELIVERY_LABELS: Record<DeliveryMode, string> = {
  pr: "Branch + PR + merge",
  local_merge: "Branch + merge locale",
  push: "Commit + push",
  commit: "Solo commit",
};

const DELIVERY_HELP: Record<DeliveryMode, string> = {
  pr: "Ogni task lavora su un branch, apre una pull request e la unisce con gh.",
  local_merge: "Ogni task lavora su un branch e lo unisce in locale alla branch base, senza push.",
  push: "Commit sul branch corrente, poi pull --rebase e push.",
  commit: "Solo commit sul branch corrente: niente push né branch.",
};

const MERGE_LABELS: Record<MergeMethod, string> = {
  squash: "Squash",
  merge: "Merge commit",
  rebase: "Rebase",
};

export function deliveryLabel(mode: string): string {
  return DELIVERY_LABELS[mode as DeliveryMode] ?? mode;
}

/** Riga di spiegazione di una modalità (impostazioni). */
export function deliveryHelp(mode: DeliveryMode): string {
  return DELIVERY_HELP[mode];
}

export function mergeMethodLabel(m: string): string {
  return MERGE_LABELS[m as MergeMethod] ?? m;
}

/** Consegna effettiva di un workspace (assente = default: solo commit). */
export function effectiveDelivery(ws: { delivery?: Delivery | null }): Delivery {
  return { ...DEFAULT_DELIVERY, ...(ws.delivery ?? {}) };
}

/** La modalità lavora su branch `task/<id>` (è l'unica in cui il parallelo ha senso). */
export function usesBranches(mode: DeliveryMode): boolean {
  return mode === "pr" || mode === "local_merge";
}

/** Un worktree per task, con fino a `max_parallel` worker insieme. */
export function isParallel(d: Pick<Delivery, "mode" | "parallel">): boolean {
  return d.parallel && usesBranches(d.mode);
}

/** Consegna pronta da inviare: il parallelo vale solo con i branch. */
export function normalizeDelivery(d: Delivery): Delivery {
  return { ...d, parallel: d.parallel && usesBranches(d.mode) };
}

/** Testo accanto allo stepper dei worker quando non si è in modalità parallela. */
export const SERIAL_WORKERS_TEXT = "1 per repository";

// ---------------------------------------------------------------- stato locale

/** Stesso id di workspace? (evento `board-update` di un'altra board: ignorato) */
export function isBoardOf(board: Pick<BoardView, "workspace_id">, workspaceId: string): boolean {
  return board.workspace_id === workspaceId;
}

/** Board vuota segnaposto (prima del caricamento o con il backend assente). */
export function emptyBoard(workspaceId: string): BoardView {
  return {
    workspace_id: workspaceId,
    rev: 0,
    max_parallel: 3,
    paused: false,
    updated_at: 0,
    tasks: [],
    proposals: [],
    settings_proposals: [],
    schedule_blocked: false,
    delivery: { ...DEFAULT_DELIVERY },
  };
}

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
