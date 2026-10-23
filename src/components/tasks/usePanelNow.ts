import { useEffect, useState } from "react";

/** Intervallo di aggiornamento dei tempi trascorsi: i secondi non servono. */
export const PANEL_TICK_MS = 30_000;

/**
 * Istante corrente, aggiornato ogni ~30 s finché `active` (un solo timer per
 * pannello, fermato allo smontaggio o quando non serve).
 */
export function usePanelNow(active: boolean, intervalMs = PANEL_TICK_MS): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [active, intervalMs]);
  return now;
}
