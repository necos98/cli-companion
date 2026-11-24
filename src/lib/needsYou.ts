// "Serve a te": tutto ciò che nel pannello Task aspetta una decisione o un
// intervento dell'utente, nell'ordine in cui il pannello lo mostra. Funzioni
// pure a zero dipendenze, importabili sia dall'app sia dai test `node --test`.

import { displayStatus } from "./board.ts";
import type { BoardView, Proposal, SettingsProposalView, Task } from "./board.ts";
import { failureReason } from "./taskPanel.ts";

export type NeedsYouItem =
  | { kind: "proposal"; proposal: Proposal }
  | { kind: "settings"; proposal: SettingsProposalView }
  | { kind: "attention"; task: Task; question: boolean }
  | {
      kind: "failed";
      task: Task;
      /** Motivo su una riga ("" se non noto). */
      reason: string;
      /** Titoli dei task in coda che questo fallimento blocca. */
      blocking: string[];
    };

type Source = Pick<BoardView, "tasks" | "proposals" | "settings_proposals">;

/** I task in coda bloccati da `taskId` (campo `blocked_by` della vista), per titolo. */
function blockedTitles(tasks: readonly Task[], taskId: string): string[] {
  return tasks.filter((t) => t.status === "queued" && t.blocked_by.includes(taskId)).map((t) => t.title);
}

/**
 * Proposte di task, proposte di impostazioni, task in corso che aspettano l'utente (domande prima dell'attenzione) e
 * task falliti, nell'ordine della board dentro ogni gruppo.
 */
export function needsYou(board: Source): NeedsYouItem[] {
  const items: NeedsYouItem[] = [];
  for (const proposal of board.proposals) items.push({ kind: "proposal", proposal });
  for (const proposal of board.settings_proposals) items.push({ kind: "settings", proposal });
  const waiting = board.tasks.flatMap((task) => {
    const shown = displayStatus(task);
    return shown === "question" || shown === "attention" ? [{ task, question: shown === "question" }] : [];
  });
  waiting.sort((a, b) => Number(b.question) - Number(a.question));
  for (const { task, question } of waiting) items.push({ kind: "attention", task, question });
  for (const task of board.tasks) {
    if (task.status !== "failed") continue;
    items.push({
      kind: "failed",
      task,
      reason: failureReason(task),
      blocking: blockedTitles(board.tasks, task.id),
    });
  }
  return items;
}

/** Chiave stabile di una voce (per React). */
export function needsYouKey(item: NeedsYouItem): string {
  switch (item.kind) {
    case "proposal":
    case "settings":
      return `${item.kind}:${item.proposal.id}`;
    default:
      return `${item.kind}:${item.task.id}`;
  }
}
