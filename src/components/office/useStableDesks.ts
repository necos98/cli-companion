import { useMemo, useRef } from "react";
import { assignDesks } from "../../lib/office";
import type { DeskMap, OfficeSession } from "../../lib/office";

/**
 * Le scrivanie dei bot, stabili per tutta la vita dell'ufficio aperto: chi c'era
 * tiene la sua, i tab chiusi la liberano, i nuovi prendono la prima libera.
 * Si azzerano a ogni apertura (il componente si rimonta).
 */
export function useStableDesks(sessions: readonly OfficeSession[]): DeskMap {
  const desks = useRef<DeskMap>(new Map());
  return useMemo(() => {
    desks.current = assignDesks(desks.current, sessions);
    return desks.current;
  }, [sessions]);
}
