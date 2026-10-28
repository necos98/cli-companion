// Task di review: etichette, verdetto e regole del form. Funzioni pure a zero
// dipendenze (solo tipi di board.ts), importabili sia dall'app sia dai test
// `node --test`.

import type { ReviewVerdict, Task, TaskKind, TaskSpec } from "./board.ts";

export const REVIEW_LABEL = "Review";
export const REVIEW_OPTION_LABEL = "Task di revisione";
export const REVIEW_OPTION_HELP =
  "Verifica i task da cui dipende e ne riporta l'esito (Verificato / Problemi trovati).";
export const REVIEW_NEEDS_DEPENDENCY = "Una review deve dipendere da almeno un task (quelli che verifica)";
export const FINDINGS_TITLE = "Problemi trovati";

type Kinded = { kind?: TaskKind | null };

/** Il task (o la specifica di una proposta) è una review; `kind` assente = work. */
export function isReview(item: Kinded): boolean {
  return item.kind === "review";
}

export interface VerdictBadge {
  verdict: ReviewVerdict;
  label: string;
  /** Classi di colore: avviso (non errore) per `issues`, successo per `passed`. */
  className: string;
}

const VERDICT_BADGES: Record<ReviewVerdict, VerdictBadge> = {
  passed: { verdict: "passed", label: "Verificato", className: "text-status-finished" },
  issues: { verdict: "issues", label: "Problemi trovati", className: "text-status-attention" },
};

/** Badge del verdetto: solo per una review `done` il cui report ne ha uno. */
export function verdictBadge(task: Pick<Task, "kind" | "status" | "report">): VerdictBadge | null {
  if (!isReview(task) || task.status !== "done") return null;
  const verdict = task.report?.verdict;
  return verdict ? (VERDICT_BADGES[verdict] ?? null) : null;
}

/** Findings da mostrare nel dettaglio: solo per una review con verdetto `issues`. */
export function reviewFindings(task: Pick<Task, "kind" | "status" | "report">): string[] {
  if (verdictBadge(task)?.verdict !== "issues") return [];
  return (task.report?.findings ?? []).filter((f) => f.trim() !== "");
}

/** Una review richiede almeno una dipendenza (come il backend). */
export function reviewDependencyError(kind: TaskKind | undefined, dependsOn: string[]): string | undefined {
  return kind === "review" && dependsOn.length === 0 ? REVIEW_NEEDS_DEPENDENCY : undefined;
}

/** Intestazione di una proposta: dal reviewer (con il task di origine) o da Task Intake. */
export function proposalHeading(
  proposal: { source_task?: string | null },
  tasks: Pick<Task, "id" | "title">[],
): { title: string; origin: string | null } {
  const source = proposal.source_task;
  if (!source) return { title: "Proposta da Task Intake", origin: null };
  return { title: "Proposta dal reviewer", origin: tasks.find((t) => t.id === source)?.title ?? source };
}

/** Specifiche di review in una proposta (per il badge sulle righe). */
export function reviewSpecIds(specs: Pick<TaskSpec, "id" | "kind">[]): Set<string> {
  return new Set(specs.filter(isReview).map((s) => s.id));
}
