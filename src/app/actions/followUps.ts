// Scelte dell'utente sui follow-up di un goal chiuso (docs/GOALS.MD, "Follow-up"): scartare una voce o mandarla
// all'Intake. Lo stato lo scrive il backend; nessuna voce diventa un task da sola.

import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { BoardView } from "../../lib/board";
import { followUpRequest } from "../../lib/followUps";
import type { FollowUp } from "../../lib/followUps";
import type { GoalView } from "../../lib/goals";
import { noticeStore } from "../../lib/notices";
import { api } from "../../lib/tauri";
import { openIntake } from "./tabs";

const INTAKE_MISSING = "Task Intake non è disponibile: la voce resta aperta.";

/** Scarta il follow-up `index` del goal. */
export function dismissFollowUp(workspaceId: string, goalId: string, index: number): Promise<BoardView> {
  return api.goalSetFollowUp(workspaceId, goalId, index, "dismissed");
}

/**
 * Manda il follow-up all'Intake: il tab del Bot Intake non accetta un testo iniziale, quindi la richiesta (testo della
 * voce e goal d'origine) va negli appunti e l'utente la incolla nel tab che si apre; lo avvisa un messaggio. La voce
 * passa a `promoted` solo se gli appunti e l'Intake hanno funzionato.
 */
export async function promoteFollowUp(
  workspaceId: string,
  goal: Pick<GoalView, "id" | "title">,
  index: number,
  followUp: FollowUp,
): Promise<BoardView> {
  await writeText(followUpRequest(goal, followUp));
  if (!openIntake(workspaceId)) throw new Error(INTAKE_MISSING);
  noticeStore.push({
    key: "follow-up:copied",
    level: "warning",
    text: "Richiesta copiata negli appunti: incollala nel tab Task Intake che si apre.",
  });
  return api.goalSetFollowUp(workspaceId, goal.id, index, "promoted");
}
