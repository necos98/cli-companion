import { useCallback, useEffect, useRef, useState } from "react";
import type { BoardView } from "../../lib/board";

/** Parte della richiesta di apertura che sceglie la selezione. */
interface SelectionIntent {
  taskId?: string;
}

/**
 * Selezione del Manager: un task. Segue le richieste che arrivano da fuori (`intentNonce` cambia a ogni richiesta)
 * e si azzera quando il task sparisce dalla board, ma solo se lo si era già visto: una selezione arrivata prima
 * della board che lo contiene resta in attesa.
 */
export function useManagerSelection(board: BoardView | null, intent: SelectionIntent | null, intentNonce?: number) {
  const [taskId, setTaskId] = useState<string | null>(() => intent?.taskId ?? null);

  useEffect(() => {
    if (intent?.taskId) setTaskId(intent.taskId);
  }, [intentNonce]); // eslint-disable-line react-hooks/exhaustive-deps

  const seen = useRef<string | null>(null);
  useEffect(() => {
    if (!board || !taskId) return;
    if (board.tasks.some((t) => t.id === taskId)) seen.current = taskId;
    else if (seen.current === taskId) {
      seen.current = null;
      setTaskId(null);
    }
  }, [board, taskId]);

  const selectTask = useCallback((id: string) => setTaskId(id), []);
  const clear = useCallback(() => setTaskId(null), []);

  return { taskId, selectTask, clear };
}
