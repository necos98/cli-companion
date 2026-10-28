// Visualizzazione dell'esito degli E2E di un task o di un repository.
import { formatDuration } from "./board.ts";
import type { E2eRun, E2eStatus, E2eTotals } from "./board.ts";

/** Caratteri mostrati dell'hash del commit. */
const SHORT_COMMIT = 7;

export const E2E_STATUS: Record<E2eStatus, { label: string; className: string }> = {
  running: { label: "in corso", className: "text-muted-foreground" },
  passed: { label: "superati", className: "text-status-finished" },
  failed: { label: "falliti", className: "text-destructive" },
  error: { label: "errore", className: "text-destructive" },
};

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

function totalsText(t: E2eTotals): string {
  const parts = [
    plural(t.passed, "superato", "superati"),
    plural(t.failed, "fallito", "falliti"),
    plural(t.skipped, "saltato", "saltati"),
  ];
  if (t.flaky) parts.push(plural(t.flaky, "instabile", "instabili"));
  return `${t.tests} test: ${parts.join(", ")}`;
}

/**
 * Stato dopo la risposta di `e2eRunRepo`: se un evento `e2e-update` ha gia' portato
 * lo stato della stessa esecuzione (anche conclusa, se e' finita prima della risposta), non lo si sovrascrive.
 */
export function afterRunResponse(prev: E2eRun | null, response: E2eRun): E2eRun {
  return prev?.run_id === response.run_id ? prev : response;
}

/** Riga di riepilogo: "12 test: 11 superati, 1 fallito, 0 saltati · 1m · abc1234". Vuota se non c'è nulla da dire. */
export function e2eSummary(run: E2eRun): string {
  const parts: string[] = [];
  if (run.totals) parts.push(totalsText(run.totals));
  if (run.duration_ms !== undefined) parts.push(formatDuration(run.duration_ms));
  if (run.code) parts.push(run.code.commit.slice(0, SHORT_COMMIT));
  return parts.join(" · ");
}
