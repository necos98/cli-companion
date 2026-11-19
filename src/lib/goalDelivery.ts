// Consegna di un goal: Bot che la esegue e branch che nasce con il goal.
// Funzioni pure a zero dipendenze (solo tipi), importabili sia dall'app sia dai test `node --test`.

import type { AgentBot } from "../bindings/AgentBot.ts";
import type { GoalView } from "../bindings/GoalView.ts";

/** Bot che esegue la consegna se il goal non ne sceglie uno (come `DEFAULT_DELIVER_BOT` del backend). */
export const DEFAULT_DELIVER_BOT = "general";

/** Bot scelto per la consegna del goal. */
export function deliverBotId(goal: Pick<GoalView, "deliver_bot">): string {
  return goal.deliver_bot ?? DEFAULT_DELIVER_BOT;
}

export interface DeliverBotChoice {
  id: string;
  label: string;
}

/**
 * Bot fra cui scegliere: accesi e con `work` (la consegna esegue le istruzioni del tipo `deliver`
 * con un Bot di lavoro). Il Bot già scelto resta in lista anche se spento o cancellato.
 */
export function deliverBotChoices(bots: readonly AgentBot[], current: string): DeliverBotChoice[] {
  const choices = bots.filter((b) => b.enabled && b.kinds.includes("work")).map((b) => ({ id: b.id, label: b.name }));
  if (!choices.some((c) => c.id === current)) {
    choices.unshift({ id: current, label: bots.find((b) => b.id === current)?.name ?? current });
  }
  return choices;
}

/** Branch che nasce con un goal. */
export function goalBranchName(goalId: string): string {
  return `goal/${goalId}`;
}

/** Etichetta breve del link di una PR: "PR #12" se l'indirizzo la contiene, altrimenti l'indirizzo. */
export function prLinkLabel(url: string): string {
  const m = /\/pull\/(\d+)/.exec(url);
  return m ? `PR #${m[1]}` : url;
}
