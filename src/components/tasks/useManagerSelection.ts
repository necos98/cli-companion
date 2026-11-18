import { useCallback, useEffect, useRef, useState } from "react";
import type { BoardView } from "../../lib/board";

/** Ciò che il dettaglio mostra: un task o un goal, mai entrambi. */
export type Selection = { kind: "task" | "goal"; id: string };

/** Parte della richiesta di apertura che sceglie la selezione. */
interface SelectionIntent {
  taskId?: string;
  goalId?: string;
}

function fromIntent(intent: SelectionIntent | null): Selection | null {
  if (intent?.taskId) return { kind: "task", id: intent.taskId };
  if (intent?.goalId) return { kind: "goal", id: intent.goalId };
  return null;
}

function exists(board: BoardView, s: Selection): boolean {
  return (s.kind === "task" ? board.tasks : board.goals).some((x) => x.id === s.id);
}

/**
 * Selezione del Manager: un task o un goal. Selezionare l'uno chiude l'altro. Segue le richieste che arrivano da
 * fuori (`intentNonce` cambia a ogni richiesta) e si azzera quando l'elemento sparisce dalla board, ma solo se lo
 * si era già visto: una selezione arrivata prima della board che lo contiene resta in attesa.
 */
export function useManagerSelection(board: BoardView | null, intent: SelectionIntent | null, intentNonce?: number) {
  const [selection, setSelection] = useState<Selection | null>(() => fromIntent(intent));

  useEffect(() => {
    const requested = fromIntent(intent);
    if (requested) setSelection(requested);
  }, [intentNonce]); // eslint-disable-line react-hooks/exhaustive-deps

  const seen = useRef<Selection | null>(null);
  useEffect(() => {
    if (!board || !selection) return;
    if (exists(board, selection)) seen.current = selection;
    else if (seen.current?.kind === selection.kind && seen.current.id === selection.id) {
      seen.current = null;
      setSelection(null);
    }
  }, [board, selection]);

  const selectTask = useCallback((id: string) => setSelection({ kind: "task", id }), []);
  const selectGoal = useCallback((id: string) => setSelection({ kind: "goal", id }), []);
  const clear = useCallback(() => setSelection(null), []);
  const taskId = selection?.kind === "task" ? selection.id : null;
  const goalId = selection?.kind === "goal" ? selection.id : null;
  return { taskId, goalId, selectTask, selectGoal, clear };
}
