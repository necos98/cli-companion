// Goal (obiettivi) della board: etichette, ordinamento, filtro, raggruppamento
// per goal e per giro. Il frontend non ricalcola fasi né attese: le legge da
// `GoalView` (docs/GOALS.MD). Funzioni pure a zero dipendenze (solo tipi),
// importabili sia dall'app sia dai test `node --test`.

import type { GoalOutcome } from "../bindings/GoalOutcome.ts";
import type { GoalPhase } from "../bindings/GoalPhase.ts";
import type { GoalReview } from "../bindings/GoalReview.ts";
import type { GoalView } from "../bindings/GoalView.ts";
import type { GoalWaiting } from "../bindings/GoalWaiting.ts";
import type { TaskView as Task } from "../bindings/TaskView.ts";

export type { GoalOutcome, GoalPhase, GoalReview, GoalView, GoalWaiting };

/** Fase di un goal in italiano. */
export const GOAL_PHASE_LABELS: Record<GoalPhase, string> = {
  executing: "In lavorazione",
  verifying: "In verifica",
  deciding: "Da decidere",
  ready: "Pronto",
  closed: "Chiuso",
};

/** Cosa aspetta l'utente, in italiano. */
export const GOAL_WAITING_LABELS: Record<GoalWaiting, string> = {
  proposal: "Una proposta da decidere",
  failed: "Un task è fallito",
  attention: "Un task richiede attenzione",
};

/** Esito della chiusura. */
export const GOAL_OUTCOME_LABELS: Record<GoalOutcome, string> = {
  delivered: "Consegnato",
  abandoned: "Chiuso a mano",
};

/** Esito dell'ultima review del goal, scritto alla chiusura. */
export const GOAL_REVIEW_LABELS: Record<GoalReview, string> = {
  passed: "Review superata",
  issues: "Review con problemi",
  none: "Nessuna review",
};

/** Valore del filtro per i task sciolti (un id di goal non può contenere "@"). */
export const NO_GOAL = "@sciolti";

/** Etichetta del gruppo dei task sciolti. */
export const LOOSE_GROUP_TITLE = "Senza goal";

type Goals = readonly GoalView[];

export function goalPhaseLabel(goal: Pick<GoalView, "phase">): string {
  return GOAL_PHASE_LABELS[goal.phase] ?? goal.phase;
}

export function goalWaitingLabels(goal: Pick<GoalView, "waiting">): string[] {
  return goal.waiting.map((w) => GOAL_WAITING_LABELS[w] ?? w);
}

/** Avanzamento "3/5": task `done` sul totale. */
export function goalProgressLabel(goal: Pick<GoalView, "tasks">): string {
  return `${goal.tasks.done}/${goal.tasks.total}`;
}

/** "giro 2" dal secondo giro in poi; null al primo. */
export function goalRoundLabel(goal: Pick<GoalView, "round">): string | null {
  return goal.round > 1 ? `giro ${goal.round}` : null;
}

export function isGoalOpen(goal: Pick<GoalView, "closed_at">): boolean {
  return goal.closed_at === undefined;
}

/** Il goal aspetta una decisione o un intervento dell'utente? */
export function isWaiting(goal: Pick<GoalView, "waiting">): boolean {
  return goal.waiting.length > 0;
}

/** Goal aperti: prima quelli che aspettano l'utente, poi dal più vecchio. Non modifica `goals`. */
export function openGoals(goals: Goals): GoalView[] {
  return goals
    .filter(isGoalOpen)
    .sort((a, b) => Number(isWaiting(b)) - Number(isWaiting(a)) || a.created_at - b.created_at);
}

/** Goal chiusi, dal più recente. */
export function closedGoals(goals: Goals): GoalView[] {
  return goals.filter((g) => !isGoalOpen(g)).sort((a, b) => (b.closed_at ?? 0) - (a.closed_at ?? 0));
}

export function findGoal(goals: Goals, id: string | undefined | null): GoalView | null {
  return id ? (goals.find((g) => g.id === id) ?? null) : null;
}

/** Titolo del goal di un task; l'id se il goal non c'è più (archiviato); null se il task è sciolto. */
export function goalLabel(goals: Goals, goalId: string | undefined | null): string | null {
  return goalId ? (findGoal(goals, goalId)?.title ?? goalId) : null;
}

// ---------------------------------------------------------------- filtro

/** Filtro per goal: null = tutti, `NO_GOAL` = solo sciolti, altrimenti l'id del goal. */
export type GoalFilter = string | null;

/** Un goal che non esiste più (chiuso e archiviato) non deve nascondere tutti i task: il filtro decade. */
export function effectiveGoalFilter(filter: GoalFilter, goals: Goals): GoalFilter {
  return filter === null || filter === NO_GOAL || findGoal(goals, filter) ? filter : null;
}

export function filterByGoal<T extends Pick<Task, "goal">>(tasks: T[], filter: GoalFilter): T[] {
  if (filter === null) return tasks;
  return tasks.filter((t) => (filter === NO_GOAL ? t.goal === undefined : t.goal === filter));
}

/** Voce del selettore del filtro. */
export interface GoalChoice {
  value: string;
  label: string;
}

/** Goal aperti, poi chiusi, poi i sciolti (solo se ci sono sia goal sia task sciolti). */
export function goalChoices(goals: Goals, tasks: Pick<Task, "goal">[]): GoalChoice[] {
  const choices = [...openGoals(goals), ...closedGoals(goals)].map((g) => ({
    value: g.id,
    label: isGoalOpen(g) ? g.title : `${g.title} (chiuso)`,
  }));
  if (choices.length > 0 && tasks.some((t) => t.goal === undefined)) {
    choices.push({ value: NO_GOAL, label: LOOSE_GROUP_TITLE });
  }
  return choices;
}

// ---------------------------------------------------------------- raggruppamento

export interface GoalGroup<T> {
  key: string;
  title: string;
  /** Il goal; null per i task sciolti o se il goal non è più nella board. */
  goal: GoalView | null;
  tasks: T[];
}

/**
 * Task raggruppati per goal (aperti, poi chiusi), con i sciolti in fondo. L'ordine dei task dentro
 * un gruppo è quello dato; i gruppi vuoti non compaiono. I task di un goal sconosciuto formano un gruppo a parte.
 */
export function groupByGoal<T extends Pick<Task, "goal">>(tasks: T[], goals: Goals): GoalGroup<T>[] {
  const byGoal = new Map<string, T[]>();
  const loose: T[] = [];
  for (const t of tasks) {
    if (t.goal === undefined) loose.push(t);
    else byGoal.set(t.goal, [...(byGoal.get(t.goal) ?? []), t]);
  }
  const groups: GoalGroup<T>[] = [];
  for (const goal of [...openGoals(goals), ...closedGoals(goals)]) {
    const members = byGoal.get(goal.id);
    if (members) groups.push({ key: goal.id, title: goal.title, goal, tasks: members });
    byGoal.delete(goal.id);
  }
  for (const [id, members] of byGoal) groups.push({ key: id, title: id, goal: null, tasks: members });
  if (loose.length > 0) groups.push({ key: NO_GOAL, title: LOOSE_GROUP_TITLE, goal: null, tasks: loose });
  return groups;
}

export interface RoundGroup<T> {
  round: number;
  tasks: T[];
}

/** Task di un goal per giro, dal primo; un task senza giro appartiene al primo. L'ordine interno è quello dato. */
export function groupByRound<T extends Pick<Task, "round">>(tasks: T[]): RoundGroup<T>[] {
  const byRound = new Map<number, T[]>();
  for (const t of tasks) {
    const round = t.round ?? 1;
    byRound.set(round, [...(byRound.get(round) ?? []), t]);
  }
  return [...byRound.entries()].sort(([a], [b]) => a - b).map(([round, members]) => ({ round, tasks: members }));
}

/** Task di un goal, nell'ordine della board. */
export function goalTasks<T extends Pick<Task, "goal">>(tasks: T[], goalId: string): T[] {
  return tasks.filter((t) => t.goal === goalId);
}
