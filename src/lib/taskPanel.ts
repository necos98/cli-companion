// Helper puri del pannello Task (accanto ai terminali) e della sua versione
// Globale: estratti, etichette, riordino per drag & drop, raggruppamento per
// workspace. Zero dipendenze di UI, importabili anche dai test `node --test`.

import { formatDuration, queuedTasks, runningTasks } from "./board.ts";
import type { BoardView, Task, TaskSpec } from "./board.ts";

// ---------------------------------------------------------------- testi

/** Una riga di testo senza simboli markdown (titoli, elenchi, enfasi, codice, link). */
export function stripMarkdown(text: string): string {
  return text
    .replace(/```[a-z0-9_-]*\n?/gi, " ")
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^\s{0,3}(#{1,6}|>|[-*+]|\d+[.)])\s+/gm, "")
    .replace(/[*_~`]+/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

/** Estratto della specifica per la card di una proposta (senza markdown, accorciato a `max`). */
export function specExcerpt(spec: string, max = 180): string {
  const flat = stripMarkdown(spec);
  return flat.length > max ? `${flat.slice(0, max - 1).trimEnd()}…` : flat;
}

/** Titoli delle dipendenze se note (task in coda o della stessa proposta), altrimenti gli id. */
export function dependencyLabels(
  ids: string[] | undefined,
  tasks: Pick<Task, "id" | "title">[],
  specs: Pick<TaskSpec, "id" | "title">[] = [],
): string[] {
  return (ids ?? []).map(
    (id) => tasks.find((t) => t.id === id)?.title ?? specs.find((t) => t.id === id)?.title ?? id,
  );
}

/** "Dipende da A, B" oppure null se non ha dipendenze. */
export function dependsOnText(labels: string[]): string | null {
  return labels.length === 0 ? null : `Dipende da ${labels.join(", ")}`;
}

/** Motivo del fallimento su una riga: nota dell'app, altrimenti il riassunto del report. */
export function failureReason(task: Pick<Task, "note" | "report">): string {
  const raw = task.note?.trim() || task.report?.summary?.trim() || "";
  return raw.replace(/\s+/g, " ");
}

/** Meta di un task in corso: "modello · tempo trascorso" (le parti assenti si omettono). */
export function runningMeta(task: Pick<Task, "model" | "started_at">, now: number): string {
  const parts: string[] = [];
  if (task.model) parts.push(task.model);
  if (task.started_at) parts.push(formatDuration(now - task.started_at));
  return parts.join(" · ");
}

/** Sottotitolo di un task in coda: "attende X" (titolo o numero) altrimenti il modello. */
export function queueSubtitle(task: Pick<Task, "model">, waitingLabel: string | null): string {
  return waitingLabel ?? task.model ?? "";
}

// ---------------------------------------------------------------- riordino

/**
 * Posizione di rilascio: `slot` k = "prima dell'elemento k" (k = n → in fondo).
 * `overIndex` è la riga sotto il puntatore, `after` se il puntatore è nella sua metà bassa.
 */
export function dropSlot(overIndex: number, after: boolean): number {
  return overIndex + (after ? 1 : 0);
}

/**
 * Argomento `before_id` di `task_move` per spostare `draggedId` nello `slot`.
 * `undefined` = nessuno spostamento da fare (stessa posizione o id sconosciuto);
 * `null` = in fondo alla coda; altrimenti l'id dell'elemento davanti a cui inserire.
 */
export function moveBeforeId(
  queueIds: string[],
  draggedId: string,
  slot: number,
): string | null | undefined {
  const from = queueIds.indexOf(draggedId);
  if (from < 0) return undefined;
  const s = Math.min(Math.max(slot, 0), queueIds.length);
  if (s === from || s === from + 1) return undefined;
  return s >= queueIds.length ? null : queueIds[s];
}

// ---------------------------------------------------------------- vista Globale

/** Attività di un workspace nel pannello Globale. */
export interface ActivityGroup<W> {
  workspace: W;
  running: Task[];
  queued: Task[];
  failed: number;
  proposals: number;
  /** Primo task in coda che può partire (senza attese); altrimenti il primo in coda. */
  next: Task | null;
}

/** Workspace con task in corso, in coda, falliti o proposte, nell'ordine dato. */
export function activityGroups<W extends { id: string }>(
  workspaces: W[],
  boards: Record<string, Pick<BoardView, "tasks" | "proposals"> | undefined>,
): ActivityGroup<W>[] {
  const groups: ActivityGroup<W>[] = [];
  for (const workspace of workspaces) {
    const board = boards[workspace.id];
    if (!board) continue;
    const running = runningTasks(board);
    const queued = queuedTasks(board);
    const failed = board.tasks.filter((t) => t.status === "failed").length;
    const proposals = board.proposals?.length ?? 0;
    if (running.length + queued.length + failed + proposals === 0) continue;
    const next = queued.find((t) => (t.waiting_on ?? []).length === 0) ?? queued[0] ?? null;
    groups.push({ workspace, running, queued, failed, proposals, next });
  }
  return groups;
}

/** Totali di tutti i gruppi (in corso, di cui attenzione, in coda, falliti, proposte). */
export function activityTotals(groups: Pick<ActivityGroup<unknown>, "running" | "queued" | "failed" | "proposals">[]): {
  running: number;
  attention: number;
  queued: number;
  failed: number;
  proposals: number;
} {
  const t = { running: 0, attention: 0, queued: 0, failed: 0, proposals: 0 };
  for (const g of groups) {
    t.running += g.running.length;
    t.attention += g.running.filter((r) => r.attention).length;
    t.queued += g.queued.length;
    t.failed += g.failed;
    t.proposals += g.proposals;
  }
  return t;
}

/** "2 in corso · 4 in coda" (+ " · 1 fallito"); le parti a zero si omettono. */
export function groupSummary(g: { running: unknown[]; queued: unknown[]; failed: number }): string {
  const parts: string[] = [];
  if (g.running.length > 0) parts.push(`${g.running.length} in corso`);
  if (g.queued.length > 0) parts.push(`${g.queued.length} in coda`);
  if (g.failed > 0) parts.push(g.failed === 1 ? "1 fallito" : `${g.failed} falliti`);
  return parts.join(" · ");
}

/** "1 proposta in attesa" / "N proposte in attesa". */
export function proposalsText(n: number): string {
  return n === 1 ? "1 proposta in attesa" : `${n} proposte in attesa`;
}
