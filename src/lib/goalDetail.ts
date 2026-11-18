// Dettaglio di un goal: fonte delle decisioni, validazione della nota e cosa
// elimina la chiusura. Funzioni pure a zero dipendenze (solo tipi), importabili
// sia dall'app sia dai test `node --test`.

import type { DecisionSource } from "../bindings/DecisionSource.ts";
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
