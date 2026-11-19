// Correzione automatica di un goal (docs/GOALS.MD, "Giri e correzione automatica"): le scelte dell'utente, il
// motivo per cui una proposta di correzione resta da decidere e i giri accettati in automatico. Il frontend non
// decide nulla: legge `auto_fix`, `auto_rounds` e `auto_fix_hold` scritti dal backend. Funzioni pure a zero
// dipendenze (solo tipi), importabili sia dall'app sia dai test `node --test`.

import type { AutoFixHold } from "../bindings/AutoFixHold.ts";
import type { GoalView } from "../bindings/GoalView.ts";
import type { Proposal } from "../bindings/Proposal.ts";

export type { AutoFixHold };

/** Tetto dei giri di correzione automatica (come `MAX_AUTO_FIX_ROUNDS` del backend). */
export const MAX_AUTO_FIX_ROUNDS = 3;

/** Testo della card in "Serve a te" per una correzione ferma al tetto. */
export const ROUND_LIMIT_NOTE = "Giri automatici finiti";

export interface AutoFixChoice {
  value: number;
  label: string;
}

/** "no", "1 giro", "2 giri", "3 giri". */
export function autoFixLabel(rounds: number): string {
  if (rounds <= 0) return "no";
  return rounds === 1 ? "1 giro" : `${rounds} giri`;
}

/** Le scelte del selettore "Correzioni automatiche": da nessuna al tetto. */
export function autoFixChoices(): AutoFixChoice[] {
  return Array.from({ length: MAX_AUTO_FIX_ROUNDS + 1 }, (_, value) => ({ value, label: autoFixLabel(value) }));
}

/** Giri automatici già usati dal goal sul suo tetto: "1/2"; null se la correzione automatica è spenta. */
export function autoFixUsageLabel(goal: Pick<GoalView, "auto_fix" | "auto_rounds">): string | null {
  const max = goal.auto_fix.max_rounds;
  return max > 0 ? `${Math.min(goal.auto_rounds.length, max)}/${max}` : null;
}

/** Il giro `round` del goal è stato accettato dall'app, non dall'utente? */
export function isAutomaticRound(goal: Pick<GoalView, "auto_rounds">, round: number): boolean {
  return goal.auto_rounds.includes(round);
}

/** Perché la proposta non è stata accettata in automatico, in italiano. */
export function autoFixHoldText(hold: AutoFixHold): string {
  switch (hold.reason) {
    case "not_from_review":
      return "non viene dalla review del goal";
    case "kind":
      return `il task ${hold.task} è di tipo ${hold.kind}: entrano da soli solo lavoro e review`;
    case "repo":
      return `il task ${hold.task} lavora in ${hold.repo === "." ? "un repository" : hold.repo} che il goal non usa`;
    case "bot":
      return `il task ${hold.task} usa ${hold.bot}, un Bot spento di serie`;
    case "invalid":
      return `validazione fallita: ${hold.error}`;
  }
}

/**
 * Nota della card di una proposta sulla correzione automatica: il tetto raggiunto ("giri automatici finiti") o il
 * motivo per cui non è entrata da sola; null se la proposta non la riguarda (correzione automatica spenta, proposta
 * dell'Intake, goal sconosciuto).
 */
export function proposalAutoFixNote(
  proposal: Pick<Proposal, "source_task" | "goal_id" | "auto_fix_hold">,
  goals: readonly Pick<GoalView, "id" | "round" | "auto_fix">[],
): string | null {
  if (proposal.auto_fix_hold) return `Non accettata in automatico: ${autoFixHoldText(proposal.auto_fix_hold)}`;
  if (!proposal.source_task || !proposal.goal_id) return null;
  const goal = goals.find((g) => g.id === proposal.goal_id);
  const max = goal?.auto_fix.max_rounds ?? 0;
  if (!goal || max === 0 || goal.round - 1 < max) return null;
  return `${ROUND_LIMIT_NOTE} (${max}/${max}): decidi tu`;
}

/** Chi ha accettato il giro `round` (dal secondo, i giri di correzione): l'app o l'utente; null per il primo. */
export function roundAcceptanceLabel(goal: Pick<GoalView, "auto_rounds">, round: number): string | null {
  if (round <= 1) return null;
  return isAutomaticRound(goal, round) ? "accettato in automatico" : "accettato da te";
}
