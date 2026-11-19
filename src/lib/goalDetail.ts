// Dettaglio di un goal: fonte delle decisioni, validazione della nota e cosa
// elimina la chiusura. Funzioni pure a zero dipendenze (solo tipi), importabili
// sia dall'app sia dai test `node --test`.

import type { DecisionSource } from "../bindings/DecisionSource.ts";
import type { GoalView } from "../bindings/GoalView.ts";
import type { BoardView, Task } from "./board.ts";

/** Lunghezza massima di una nota (come il backend). */
export const GOAL_NOTE_MAX = 500;

/** Chi ha scritto una voce del registro delle decisioni, in italiano. */
export const DECISION_SOURCE_LABELS: Record<DecisionSource, string> = {
  intake: "Intake",
  user: "Tu",
  app: "App",
};

export function decisionSourceLabel(source: DecisionSource): string {
  return DECISION_SOURCE_LABELS[source] ?? source;
}

/** Errore della nota (vuoto = valida): da 1 a `GOAL_NOTE_MAX` caratteri, spazi ai bordi esclusi. */
export function goalNoteError(text: string): string | undefined {
  const length = [...text.trim()].length;
  if (length === 0) return "La nota è vuota";
  if (length > GOAL_NOTE_MAX) return `La nota può avere al massimo ${GOAL_NOTE_MAX} caratteri`;
  return undefined;
}

/** Cosa succede chiudendo un goal: task e proposte che spariscono, task in corso che lo impediscono. */
export interface CloseImpact {
  /** Task `queued` e `failed` del goal: vengono eliminati. */
  removed: Task[];
  /** Task `running`: il backend rifiuta la chiusura finché non finiscono. */
  running: Task[];
  /** Titoli delle proposte pendenti del goal: vengono scartate (uno per proposta). */
  proposals: string[];
}

export function goalCloseImpact(board: Pick<BoardView, "tasks" | "proposals">, goalId: string): CloseImpact {
  const own = board.tasks.filter((t) => t.goal === goalId);
  return {
    removed: own.filter((t) => t.status === "queued" || t.status === "failed"),
    running: own.filter((t) => t.status === "running"),
    proposals: board.proposals
      .filter((p) => p.goal_id === goalId)
      .map((p) => p.tasks.map((t) => t.title).join(", ") || p.id),
  };
}

/** La chiusura è bloccata finché un task del goal è in corso. */
export function closeBlocked(impact: Pick<CloseImpact, "running">): boolean {
  return impact.running.length > 0;
}

/** Una riga dei branch di un goal: repository, nome del branch e base da cui è nato. */
export interface BranchRow {
  repo: string;
  label: string;
  branch: string;
  base: string;
}

/** Nome del repository: la root del workspace (chiave ".") o la cartella relativa. */
export function repoLabel(repo: string): string {
  return repo === "." ? "root del workspace" : repo;
}

/** Branch del goal per repository, con la root per prima e poi in ordine alfabetico. */
export function goalBranchRows(goal: Pick<GoalView, "branches">): BranchRow[] {
  return Object.entries(goal.branches ?? {})
    .map(([repo, b]) => ({ repo, label: repoLabel(repo), branch: b.branch, base: b.base }))
    .sort((a, b) => (a.repo === "." ? -1 : b.repo === "." ? 1 : a.repo.localeCompare(b.repo)));
}

/** I branch si eliminano solo a mano: goal chiuso senza consegna (`abandoned`) e almeno un branch. */
export function canDeleteBranches(goal: Pick<GoalView, "branches" | "closed_at" | "outcome">): boolean {
  return goal.closed_at !== undefined && goal.outcome === "abandoned" && Object.keys(goal.branches ?? {}).length > 0;
}
