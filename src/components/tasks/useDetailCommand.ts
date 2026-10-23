import { useCallback, useEffect, useRef, useState } from "react";
import type { BoardView } from "../../lib/board";

/**
 * Esegue un comando Tauri che restituisce la board: la passa a `onBoard`,
 * tiene `busy` ed espone l'errore del backend da mostrare in linea.
 * `run` risolve a true se il comando è riuscito.
 */
export function useCommand(onBoard: (board: BoardView) => void) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const run = useCallback(
    async (command: () => Promise<BoardView>): Promise<boolean> => {
      setBusy(true);
      setError(null);
      try {
        onBoard(await command());
        return true;
      } catch (e) {
        if (alive.current) setError(String(e));
        return false;
      } finally {
        if (alive.current) setBusy(false);
      }
    },
    [onBoard],
  );

  const clearError = useCallback(() => setError(null), []);
  return { run, busy, error, clearError };
}
