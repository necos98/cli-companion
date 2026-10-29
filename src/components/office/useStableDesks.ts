import { useMemo, useRef } from "react";
import { assignDesks } from "../../lib/office";
import type { DeskMap } from "../../lib/office";

interface DeskTab {
  key: string;
  workspace: { id: string };
}

/**
 * Le scrivanie dei bot, stabili per tutta la vita del tab: chi c'era tiene la
 * sua, i tab chiusi la liberano, i nuovi prendono la prima libera. Va chiamato
 * da chi vive quanto l'App (non dall'Ufficio) con tutti i tab aperti, anche ad
 * Ufficio chiuso. Nulla cambiato -> stessa istanza.
 */
export function useStableDesks(tabs: readonly DeskTab[]): DeskMap {
  const desks = useRef<DeskMap>(new Map());
  return useMemo(() => {
    const sessions = tabs.map((t) => ({ key: t.key, workspaceId: t.workspace.id }));
    desks.current = assignDesks(desks.current, sessions);
    return desks.current;
  }, [tabs]);
}
