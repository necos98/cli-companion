// Coda dei task di un workspace ("Tasks Manager"): tipi del contratto
// (docs/BOARD.MD), stati con etichette e colori, selettori, filtri, formati di
// durata e consegna. Funzioni pure a zero dipendenze (solo automation.ts),
// importabili sia dall'app sia dai test `node --test`.

import { statusWord, truncateReport } from "./automation.ts";
import type { BoardView } from "../bindings/BoardView.ts";
import type { Bounds } from "../bindings/Bounds.ts";
import type { Delivery } from "../bindings/Delivery.ts";
import type { FailReason } from "../bindings/FailReason.ts";
import type { DeliveryMode } from "../bindings/DeliveryMode.ts";
import type { MergeMethod } from "../bindings/MergeMethod.ts";
import type { TaskAction } from "../bindings/TaskAction.ts";
import type { TaskStatus } from "../bindings/TaskStatus.ts";
import type { TaskView as Task } from "../bindings/TaskView.ts";

// ---------------------------------------------------------------- tipi

// I tipi del contratto sono generati dai DTO Rust (`src/bindings`): qui si
// ri-esportano con i nomi che i consumatori importano da questo modulo.
export type { BoardView, Delivery, DeliveryMode, MergeMethod, Task, TaskAction, TaskStatus };
export type { E2eFailure } from "../bindings/E2eFailure.ts";
export type { E2eRun } from "../bindings/E2eRun.ts";
export type { E2eStatus } from "../bindings/E2eStatus.ts";
export type { E2eTotals } from "../bindings/E2eTotals.ts";
export type { NewTaskSpec } from "../bindings/NewTaskSpec.ts";
export type { Notice as TaskNotifyEvent } from "../bindings/Notice.ts";
export type { Proposal } from "../bindings/Proposal.ts";
export type { RepoPatch } from "../bindings/RepoPatch.ts";
export type { SettingsPatch } from "../bindings/SettingsPatch.ts";
export type { SettingsProposalView } from "../bindings/SettingsProposalView.ts";
export type { TaskKind } from "../bindings/TaskKind.ts";
export type { TaskPatch } from "../bindings/TaskPatch.ts";
export type { TaskReport } from "../bindings/TaskReport.ts";
export type { TaskSpec } from "../bindings/TaskSpec.ts";
export type { TestRun as TaskTestResult } from "../bindings/TestRun.ts";
export type { Verdict as ReviewVerdict } from "../bindings/Verdict.ts";

/** Stato mostrato: i quattro stati più "Richiede attenzione" (running con `attention`) e "In attesa di risposta" (running con una domanda pendente). */
export type DisplayStatus = TaskStatus | "attention" | "question";

// ---------------------------------------------------------------- stati

/** Le varianti di `U` nell'ordine delle chiavi: una variante mancante o nuova in Rust non compila. */
function variantsOf<U extends string>(order: Record<U, true>): U[] {
  return Object.keys(order) as U[];
}

/** Ordine dei quattro stati (legenda, filtri, conteggi). */
export const STATUSES = variantsOf<TaskStatus>({ queued: true, running: true, done: true, failed: true });

interface StatusMeta {
  label: string;
  /** Colore del testo. */
  text: string;
  /** Colore del punto/badge. */
  dot: string;
  /** Bordo del nodo nel grafo. */
  border: string;
}

/**
 * Da fare = grigio, In corso = ambra, Richiede attenzione = arancione, Fatto =
 * verde, Fallito = rosso. Il blu (primary) è solo per ciò che l'utente può
 * decidere: azioni, proposte e domande dei worker (In attesa di risposta).
 */
const META: Record<DisplayStatus, StatusMeta> = {
  queued: {
    label: "Da fare",
    text: "text-muted-foreground",
    dot: "bg-muted-foreground",
    border: "border-border-strong",
  },
  running: {
    label: "In corso",
    text: "text-status-running",
    dot: "bg-status-running",
    border: "border-status-running/70",
  },
  attention: {
    label: "Richiede attenzione",
    text: "text-status-attention",
    dot: "bg-status-attention",
    border: "border-status-attention/70",
  },
  question: {
    label: "In attesa di risposta",
    text: "text-primary",
    dot: "bg-primary",
    border: "border-primary/60",
  },
  done: {
    label: "Fatto",
    text: "text-status-finished",
    dot: "bg-status-finished",
    border: "border-status-finished/50",
  },
  failed: {
    label: "Fallito",
    text: "text-destructive",
    dot: "bg-destructive",
    border: "border-destructive/70",
  },
};

const FALLBACK = META.queued;

function metaOf(s: string): StatusMeta {
  return META[s as DisplayStatus] ?? FALLBACK;
}

/** Stato da mostrare: un task in corso con una domanda pendente è "question", uno che aspetta l'utente per altro è "attention". */
export function displayStatus(task: Pick<Task, "status" | "attention" | "question">): DisplayStatus {
  if (task.status !== "running") return task.status;
  if (hasPendingQuestion(task)) return "question";
  return task.attention ? "attention" : "running";
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

export interface TaskCounts extends Record<TaskStatus, number> {
  proposals: number;
  /** In corso che richiedono attenzione (sottoinsieme di `running`, senza quelli con una domanda). */
  attention: number;
  /** In corso con una domanda del worker senza risposta (sottoinsieme di `running`). */
  question: number;
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
    question: 0,
    total: 0,
  };
  for (const t of board?.tasks ?? []) {
    c.total++;
    if (t.status in c) c[t.status]++;
    const shown = displayStatus(t);
    if (shown === "attention") c.attention++;
    else if (shown === "question") c.question++;
  }
  return c;
}

/** In corso che aspettano l'utente (le domande dei worker si contano a parte, in `taskCounts`). */
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

/** Limita un valore all'intervallo consentito dal backend. */
export function clampToBounds(n: number, { min, max }: Bounds): number {
  if (!Number.isFinite(n)) return min;
  return Math.min(max, Math.max(min, Math.round(n)));
}

// ---------------------------------------------------------------- azioni

export const ACTION_LABELS: Record<TaskAction, string> = {
  retry: "Riprova",
  mark_done: "Segna come fatto",
  stop: "Ferma",
  delete: "Elimina",
  delete_cascade: "Elimina tutti",
  to_top: "In cima alla coda",
};

/** "Apri terminale" ha senso solo se il task ha un tab worker. */
export function canOpenTerminal(task: Pick<Task, "session">): boolean {
  return typeof task.session === "string" && task.session !== "";
}

/** Il worker ha fatto una domanda all'utente e aspetta la risposta. */
export function hasPendingQuestion(task: Pick<Task, "question">): boolean {
  return Boolean(task.question);
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
  const w = task.waiting_on;
  if (task.status !== "queued" || w.length === 0) return null;
  if (w.length === 1) {
    const title = tasks.find((t) => t.id === w[0])?.title;
    return `attende ${title ?? w[0]}`;
  }
  return `attende ${w.length} task`;
}

/** "bloccato da X" (un solo task: il suo titolo) o "bloccato da N task"; null se non è bloccato. */
export function blockedByLabel(
  task: Pick<Task, "blocked_by" | "status">,
  tasks: Pick<Task, "id" | "title">[],
): string | null {
  const b = task.blocked_by;
  if (task.status !== "queued" || b.length === 0) return null;
  if (b.length === 1) {
    const title = tasks.find((t) => t.id === b[0])?.title;
    return `bloccato da ${title ?? b[0]}`;
  }
  return `bloccato da ${b.length} task`;
}

/** Perché un task è in coda e non parte: "bloccato da …" ha la precedenza su "attende …". */
export function queueHoldLabel(
  task: Pick<Task, "waiting_on" | "blocked_by" | "status">,
  tasks: Pick<Task, "id" | "title">[],
): string | null {
  return blockedByLabel(task, tasks) ?? waitingOnLabel(task, tasks);
}

/** Il motivo del fallimento in italiano, nelle parole dell'utente. */
export const FAIL_REASON_LABELS: Record<FailReason, string> = {
  worker_failed: "Il worker ha rinunciato",
  worker_exited: "Worker uscito senza esito",
  stopped: "Fermato dall'utente",
  interrupted: "App riavviata",
  start_failed: "Avvio fallito",
};

/** Motivo di un task fallito; null se non è fallito o il motivo non è noto (task vecchi). */
export function failReasonLabel(task: Pick<Task, "status" | "fail_reason">): string | null {
  if (task.status !== "failed" || !task.fail_reason) return null;
  return FAIL_REASON_LABELS[task.fail_reason] ?? null;
}

/** Cosa fermare in evidenza di un task: perché è fallito o da chi è bloccato; null altrimenti. */
export function issueLabel(
  task: Pick<Task, "status" | "fail_reason" | "blocked_by">,
  tasks: Pick<Task, "id" | "title">[],
): string | null {
  return failReasonLabel(task) ?? blockedByLabel(task, tasks);
}

/** Spiegazione della modalità libera (cartella senza git), per il tooltip. */
export const NO_GIT_HELP =
  "Modalità libera: la cartella non è un repository git, il worker lavora direttamente lì senza branch né commit.";

/** Il task gira in modalità libera (cartella senza git): niente branch, worktree o commit. */
export function isNoGit(task: Pick<Task, "no_git">): boolean {
  return task.no_git === true;
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
  const haystack = [task.title, task.task, task.report?.summary, task.note, task.last_report, task.id];
  return haystack.some((h) => typeof h === "string" && h.toLowerCase().includes(q));
}

export interface TaskFilter {
  query: string;
  /** Stati ammessi; vuoto = tutti. */
  statuses: TaskStatus[];
  /** Repository ammessi ("." = la root); vuoto = tutti. */
  repos: string[];
  /** Agenti ammessi (id dell'integrazione); vuoto = tutti. */
  agents: string[];
}

export const EMPTY_FILTER: TaskFilter = { query: "", statuses: [], repos: [], agents: [] };

/** C'è almeno un filtro attivo? (per "Azzera filtri") */
export function isFiltering(f: TaskFilter): boolean {
  return f.query.trim() !== "" || f.statuses.length > 0 || f.repos.length > 0 || f.agents.length > 0;
}

/** Applica ricerca, stati, repository e agenti (l'ordine dei task non cambia). */
export function filterTasks(tasks: Task[], f: TaskFilter): Task[] {
  return tasks.filter(
    (t) =>
      (f.statuses.length === 0 || f.statuses.includes(t.status)) &&
      (f.repos.length === 0 || f.repos.includes(t.repo || ".")) &&
      (f.agents.length === 0 || f.agents.includes(t.agent)) &&
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

export const DELIVERY_MODES = variantsOf<DeliveryMode>({ pr: true, local_merge: true, push: true, commit: true });
export const MERGE_METHODS = variantsOf<MergeMethod>({ squash: true, merge: true, rebase: true });

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
