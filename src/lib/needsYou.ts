// "Serve a te": tutto ciò che nel pannello Task aspetta una decisione o un
// intervento dell'utente, nell'ordine in cui il pannello lo mostra. Funzioni
// pure a zero dipendenze, importabili sia dall'app sia dai test `node --test`.

import { displayStatus } from "./board.ts";
import type { BoardView, Proposal, SettingsProposalView, Task } from "./board.ts";
import { findGoal, goalLabel } from "./goals.ts";
import { failureReason } from "./taskPanel.ts";

export type NeedsYouItem =
  | {
      kind: "proposal";
      proposal: Proposal;
      /** Titolo del goal a cui appartiene (o che nascerebbe); null per i task sciolti. */
      goal: string | null;
      /** Giro che la proposta di correzione aprirebbe; null se non è una correzione. */
      round: number | null;
    }
  | { kind: "settings"; proposal: SettingsProposalView }
  | { kind: "attention"; task: Task; goal: string | null; question: boolean }
  | {
      kind: "failed";
      task: Task;
      goal: string | null;
      /** Motivo su una riga ("" se non noto). */
      reason: string;
      /** Titoli dei task in coda che questo fallimento blocca. */
      blocking: string[];
    };

type Source = Pick<BoardView, "tasks" | "proposals" | "settings_proposals" | "goals">;

/** Titolo del goal di una proposta: quello nuovo che porta, o quello aperto a cui si aggiunge. */
function proposalGoal(proposal: Proposal, board: Pick<Source, "goals">): string | null {
  return proposal.goal?.title ?? goalLabel(board.goals ?? [], proposal.goal_id);
}

/** Una proposta del revisore su un goal esistente è una correzione: apre il giro successivo. */
function correctionRound(proposal: Proposal, board: Pick<Source, "goals">): number | null {
  if (!proposal.source_task) return null;
  const goal = findGoal(board.goals ?? [], proposal.goal_id);
  return goal ? goal.round + 1 : null;
}

/** I task in coda bloccati da `taskId` (campo `blocked_by` della vista), per titolo. */
function blockedTitles(tasks: readonly Task[], taskId: string): string[] {
  return tasks.filter((t) => t.status === "queued" && t.blocked_by.includes(taskId)).map((t) => t.title);
}

/**
 * Proposte di task, proposte di impostazioni, task in corso che aspettano l'utente (domande prima
 * dell'attenzione) e task falliti, nell'ordine della board dentro ogni gruppo.
 */
export function needsYou(board: Source): NeedsYouItem[] {
  const goals = board.goals ?? [];
  const items: NeedsYouItem[] = [];
  for (const proposal of board.proposals) {
    items.push({
      kind: "proposal",
      proposal,
      goal: proposalGoal(proposal, board),
      round: correctionRound(proposal, board),
    });
  }
  for (const proposal of board.settings_proposals) items.push({ kind: "settings", proposal });
  const waiting = board.tasks.flatMap((task) => {
    const shown = displayStatus(task);
    return shown === "question" || shown === "attention" ? [{ task, question: shown === "question" }] : [];
  });
  waiting.sort((a, b) => Number(b.question) - Number(a.question));
  for (const { task, question } of waiting) {
    items.push({ kind: "attention", task, goal: goalLabel(goals, task.goal), question });
  }
  for (const task of board.tasks) {
    if (task.status !== "failed") continue;
    items.push({
      kind: "failed",
      task,
      goal: goalLabel(goals, task.goal),
      reason: failureReason(task),
      blocking: blockedTitles(board.tasks, task.id),
    });
  }
  return items;
}

/** Chiave stabile di una voce (per React). */
export function needsYouKey(item: NeedsYouItem): string {
  return item.kind === "proposal" || item.kind === "settings"
    ? `${item.kind}:${item.proposal.id}`
    : `${item.kind}:${item.task.id}`;
}
