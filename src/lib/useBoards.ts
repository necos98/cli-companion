import { useCallback, useEffect, useRef, useState } from "react";
import { api, onBoardUpdate, onTaskNotify } from "./tauri";
import type { Workspace } from "./tauri";
import { mergeBoard } from "./board";
import type { BoardView } from "./board";
import { notifyIfUnfocused, notifyPlainIfUnfocused } from "./systemNotify";

/**
 * Code dei task di tutti i workspace: caricamento all'avvio (e per i workspace
 * aggiunti dopo), aggiornamenti dall'evento `board-update` e `storeBoard` per
 * le risposte dei comandi. Con il backend assente o in errore il riepilogo
 * resta vuoto; l'errore (`boardErrors`) si mostra solo nel Tasks Manager.
 */
export function useBoards(workspaces: Workspace[]) {
  const [boards, setBoards] = useState<Record<string, BoardView>>({});
  const [boardErrors, setBoardErrors] = useState<Record<string, string>>({});

  // Una risposta più vecchia di quella nota non la sostituisce (rev monotona).
  const storeBoard = useCallback((b: BoardView) => {
    setBoards((prev) => ({ ...prev, [b.workspace_id]: mergeBoard(prev[b.workspace_id], b) }));
  }, []);

  // Caricamento (o ricaricamento) di una board.
  const loadBoard = useCallback(
    (workspaceId: string) => {
      api
        .boardGet(workspaceId)
        .then((b) => {
          storeBoard(b);
          setBoardErrors((prev) => {
            if (!(workspaceId in prev)) return prev;
            const next = { ...prev };
            delete next[workspaceId];
            return next;
          });
        })
        .catch((e) => setBoardErrors((prev) => ({ ...prev, [workspaceId]: String(e) })));
    },
    [storeBoard],
  );

  // Riepilogo delle board all'avvio e per i workspace aggiunti dopo.
  const requestedRef = useRef(new Set<string>());
  useEffect(() => {
    for (const w of workspaces) {
      if (requestedRef.current.has(w.id)) continue;
      requestedRef.current.add(w.id);
      loadBoard(w.id);
    }
  }, [workspaces, loadBoard]);

  // Aggiornamenti della board dal backend.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    onBoardUpdate((b) => storeBoard(b))
      .then((u) => {
        if (cancelled) u();
        else unlisten = u;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [storeBoard]);

  return { boards, boardErrors, storeBoard, loadBoard };
}

/**
 * Notifiche di sistema dei task (`task-notify`): un task richiede attenzione o
 * è fallito. Titolo = nome del workspace, testo = quello italiano del backend.
 * Con il tab del worker ancora aperto il toast è cliccabile (porta al tab);
 * solo a finestra non in primo piano, come le altre notifiche.
 */
export function useTaskNotify(workspaces: Workspace[]) {
  const workspacesRef = useRef(workspaces);
  workspacesRef.current = workspaces;
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    onTaskNotify((e) => {
      const name = workspacesRef.current.find((w) => w.id === e.workspace_id)?.name ?? "Workspace";
      if (e.tab_id) void notifyIfUnfocused(e.tab_id, name, e.body);
      else void notifyPlainIfUnfocused(name, e.body);
    })
      .then((u) => {
        if (cancelled) u();
        else unlisten = u;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
