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

export const E2E_REQUIRED_LABEL = "E2E richiesto";
export const E2E_OPTION_LABEL = "Richiede E2E";
export const E2E_OPTION_HELP = "Il task si completa solo se gli E2E del repository passano sul codice consegnato.";
export const E2E_UNAVAILABLE_HELP =
  "Gli E2E non sono configurati per questo repository: impostali in Impostazioni → Repository.";
export const E2E_SECTION_TITLE = "Test E2E (verificati dall'app)";

export interface E2eBadge {
  label: string;
  /** Classi di colore del testo; il bordo segue il colore corrente. */
  className: string;
}

const REQUIRED_BADGE: E2eBadge = { label: E2E_REQUIRED_LABEL, className: "text-muted-foreground" };

/** Badge dell'esito dell'esecuzione: l'esito lo decide il backend, qui solo la resa. */
export function e2eRunBadge(run: Pick<E2eRun, "status" | "totals">): E2eBadge {
  const { status, totals } = run;
  switch (status) {
    case "passed":
      return {
        label: totals ? `E2E ${totals.passed}/${totals.tests}` : "E2E superati",
        className: E2E_STATUS.passed.className,
      };
    case "failed":
      return {
        label: totals?.failed ? `E2E ${plural(totals.failed, "fallito", "falliti")}` : "E2E falliti",
        className: E2E_STATUS.failed.className,
      };
    case "running":
      return { label: "E2E in corso", className: "text-status-running" };
    case "error":
      return { label: "E2E errore", className: E2E_STATUS.error.className };
  }
}

type E2eTask = { e2e?: boolean; e2e_run?: E2eRun | null; report?: { e2e?: E2eRun | null } | null };

/** Esecuzione da mostrare per un task: l'ultima del tentativo, o quella del report. */
export function taskE2eRun(task: Pick<E2eTask, "e2e_run" | "report">): E2eRun | null {
  return task.e2e_run ?? task.report?.e2e ?? null;
}

/** Badge di un task: l'esito dell'ultima esecuzione, o "E2E richiesto" se marcato e mai eseguito; niente altrimenti. */
export function e2eBadge(task: E2eTask): E2eBadge | null {
  const run = taskE2eRun(task);
  if (run) return e2eRunBadge(run);
  return task.e2e ? REQUIRED_BADGE : null;
}

/** Badge di una specifica di una proposta: ha solo il flag, nessuna esecuzione. */
export function specE2eBadge(spec: { e2e?: boolean }): E2eBadge | null {
  return spec.e2e ? REQUIRED_BADGE : null;
}

const ROOT_KEY = ".";

/** Il repository (null/vuoto/"." = la root) ha gli E2E configurati? `repos` e' `Workspace.repos`. */
export function repoHasE2e(repos: Record<string, { e2e?: unknown }> | undefined, repo: string | null | undefined): boolean {
  return Boolean(repos?.[repo || ROOT_KEY]?.e2e);
}

export interface E2eOption {
  checked: boolean;
  disabled: boolean;
  help: string;
}

/**
 * Stato della casella "Richiede E2E": attiva solo con gli E2E configurati. Un flag gia' acceso
 * resta visibile e si puo' togliere anche se la configurazione e' sparita.
 */
export function e2eOption(checked: boolean, configured: boolean): E2eOption {
  return {
    checked,
    disabled: !configured && !checked,
    help: configured || checked ? E2E_OPTION_HELP : E2E_UNAVAILABLE_HELP,
  };
}

/** Cambiare repository verso uno senza E2E spegne il flag: il backend rifiuterebbe la combinazione. */
export function withRepoE2e<T extends { repo?: string; e2e?: boolean }>(
  patch: T,
  repos: Record<string, { e2e?: unknown }> | undefined,
): T {
  if (patch.repo === undefined || repoHasE2e(repos, patch.repo)) return patch;
  return { ...patch, e2e: false };
}
