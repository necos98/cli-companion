// Follow-up dei goal chiusi (docs/GOALS.MD, "Follow-up"): il backend li raccoglie alla chiusura; qui quali
// mostrare in "Serve a te" (aperti, di goal chiusi negli ultimi 14 giorni, per goal), le etichette di stato e la
// richiesta che parte verso l'Intake. Funzioni pure a zero dipendenze (solo tipi), importabili sia dall'app sia dai
// test `node --test`.

import type { FollowUp } from "../bindings/FollowUp.ts";
import type { FollowUpState } from "../bindings/FollowUpState.ts";
import type { GoalView } from "../bindings/GoalView.ts";

export type { FollowUp, FollowUpState };

/** Giorni dopo la chiusura per cui i follow-up aperti restano in "Serve a te". */
export const FOLLOW_UP_WINDOW_DAYS = 14;

const DAY_MS = 86_400_000;

/** Stato di un follow-up in italiano. */
export const FOLLOW_UP_STATE_LABELS: Record<FollowUpState, string> = {
  open: "Aperto",
  dismissed: "Scartato",
  promoted: "Mandato all'Intake",
};

export function followUpStateLabel(state: FollowUpState): string {
  return FOLLOW_UP_STATE_LABELS[state] ?? state;
}

/** Un follow-up con la sua posizione nel goal: è l'indice che identifica la voce nel comando. */
export interface FollowUpEntry {
  index: number;
  followUp: FollowUp;
}

/** I follow-up aperti di un goal chiuso di recente. */
export interface FollowUpGroup {
  goal: GoalView;
  entries: FollowUpEntry[];
}

/** Le voci di `goal` con il loro indice, tutte o solo quelle nello stato `state`. */
export function followUpEntries(goal: Pick<GoalView, "follow_ups">, state?: FollowUpState): FollowUpEntry[] {
  return (goal.follow_ups ?? [])
    .map((followUp, index) => ({ index, followUp }))
    .filter((e) => state === undefined || e.followUp.state === state);
}

/** Il goal si è chiuso negli ultimi `FOLLOW_UP_WINDOW_DAYS` giorni (estremo incluso)? */
export function closedRecently(goal: Pick<GoalView, "closed_at">, now: number): boolean {
  return goal.closed_at !== undefined && goal.closed_at >= now - FOLLOW_UP_WINDOW_DAYS * DAY_MS;
}

/**
 * Follow-up aperti dei goal chiusi negli ultimi 14 giorni, raggruppati per goal (il chiuso più di recente per primo).
 * I goal aperti, quelli chiusi da più tempo e le voci già scartate o mandate all'Intake non compaiono.
 */
export function openFollowUpGroups(goals: readonly GoalView[], now: number): FollowUpGroup[] {
  return goals
    .filter((goal) => closedRecently(goal, now))
    .map((goal) => ({ goal, entries: followUpEntries(goal, "open") }))
    .filter((group) => group.entries.length > 0)
    .sort((a, b) => (b.goal.closed_at ?? 0) - (a.goal.closed_at ?? 0));
}

/**
 * La richiesta che l'Intake riceve per un follow-up: il testo della voce con il goal d'origine (titolo e id).
 * È testo per il modello: in inglese.
 */
export function followUpRequest(goal: Pick<GoalView, "id" | "title">, followUp: FollowUp): string {
  return [
    `Follow-up from the closed goal "${goal.title}" (id: ${goal.id}), suggested by task ${followUp.source_task}:`,
    "",
    followUp.text,
    "",
    `Plan it as a new request. Read the origin goal with get_goal ("${goal.id}") if you need its context.`,
  ].join("\n");
}
