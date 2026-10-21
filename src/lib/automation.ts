// Sessioni automatiche (worker avviati da un orchestratore): metadati del tab,
// etichette di stato, tooltip ed esclusione dalla persistenza. Funzioni pure a
// zero dipendenze, importabili sia dall'app sia dai test `node --test`
// (type-stripping). Il backend e' src-tauri/src/automation.rs.

/** Stati di un worker (vedi `list_sessions` in automation.rs). */
export type WorkerStatus =
  | "starting"
  | "running"
  | "background"
  | "needs_input"
  | "idle"
  | "completed"
  | "failed"
  | "blocked"
  | "closed";

/** Metadati di un tab automatico. `status` e' una stringa: il backend puo' aggiungerne. */
export type AutomationRole = "worker" | "orchestrator";

export interface AutomationMeta {
  /** Ruolo del tab: worker di un task o orchestratore automatico (assente = worker). */
  role?: AutomationRole;
  /** Tab dell'orchestratore che ha avviato il worker. */
  parentTabId: string;
  /** Titolo del task. */
  title: string;
  status: string;
  lastReport?: string;
  agentId: string;
  cwd?: string;
}

/** Etichetta del tab di un orchestratore automatico senza titolo. */
export const ORCHESTRATOR_LABEL = "Orchestratore automatico";

/** Normalizza il ruolo dell'evento `automation-spawn` (assente o sconosciuto = worker). */
export function normalizeRole(role: string | null | undefined): AutomationRole {
  return role === "orchestrator" ? "orchestrator" : "worker";
}

export function isOrchestrator(meta: Pick<AutomationMeta, "role"> | undefined | null): boolean {
  return meta?.role === "orchestrator";
}

/** Badge dell'intestazione: sigla e nome breve per ruolo. */
export function automationBadge(role: AutomationRole | undefined): { text: string; title: string } {
  return role === "orchestrator"
    ? { text: "ORCH", title: ORCHESTRATOR_LABEL }
    : { text: "AUTO", title: "Sessione automatica" };
}

/** Id dell'avvio integrato "Orchestratore" (vedi agents/mod.rs). */
export const ORCHESTRATOR_LAUNCH_ID = "builtin:orchestrator";

/** Etichetta del badge di un orchestratore avviato a mano. */
export const MANUAL_ORCHESTRATOR_LABEL = "Orchestratore";

/**
 * Badge dell'intestazione di un tab: ORCH per l'orchestratore automatico e per
 * quello avviato a mano (avvio integrato), AUTO per i worker, nessuno per gli
 * altri tab.
 */
export function tabBadge(tab: {
  automation?: { role?: AutomationRole } | null;
  launchId?: string;
}): { text: string; title: string; orchestrator: boolean } | null {
  if (tab.automation) {
    return { ...automationBadge(tab.automation.role), orchestrator: isOrchestrator(tab.automation) };
  }
  if (tab.launchId === ORCHESTRATOR_LAUNCH_ID) {
    return { text: "ORCH", title: MANUAL_ORCHESTRATOR_LABEL, orchestrator: true };
  }
  return null;
}

/** Lunghezza massima dell'ultimo report nel tooltip. */
export const REPORT_TOOLTIP_MAX = 240;

/** Un tab e' automatico se ha i metadati dell'automazione. */
export function isAutomated(tab: { automation?: unknown }): boolean {
  return tab.automation !== undefined && tab.automation !== null;
}

const STATUS_WORDS: Record<string, string> = {
  starting: "in avvio",
  running: "in corso",
  background: "in background",
  needs_input: "aspetta",
  idle: "fermo",
  completed: "completato",
  failed: "fallito",
  blocked: "bloccato",
  closed: "chiuso",
};

/** Classe colore (token del tema) per ogni stato. */
const STATUS_CLASSES: Record<string, string> = {
  starting: "text-muted-foreground",
  running: "text-status-running",
  background: "text-status-running",
  needs_input: "text-status-attention",
  idle: "text-muted-foreground",
  completed: "text-status-finished",
  failed: "text-destructive",
  blocked: "text-status-attention",
  closed: "text-muted-foreground",
};

/** Parola italiana dello stato; uno stato sconosciuto si mostra com'e'. */
export function statusWord(status: string): { text: string; className: string } {
  return {
    text: STATUS_WORDS[status] ?? status,
    className: STATUS_CLASSES[status] ?? "text-muted-foreground",
  };
}

/** Accorcia il testo a `max` caratteri (spazi compattati), con "…" se tagliato. */
export function truncateReport(text: string, max: number = REPORT_TOOLTIP_MAX): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 1).trimEnd()}…` : flat;
}

/**
 * Testo del tooltip dell'intestazione: titolo del task, stato, orchestratore
 * (`parentLabel`, se il suo tab esiste ancora) e ultimo report troncato.
 */
export function automationTooltip(meta: AutomationMeta, parentLabel?: string): string {
  if (isOrchestrator(meta)) {
    const lines = [ORCHESTRATOR_LABEL, `Stato: ${statusWord(meta.status).text}`];
    const report = meta.lastReport ? truncateReport(meta.lastReport) : "";
    if (report) lines.push(`Ultimo report: ${report}`);
    return lines.join("\n");
  }
  const lines = [`Sessione automatica: ${meta.title}`, `Stato: ${statusWord(meta.status).text}`];
  if (parentLabel) lines.push(`Orchestratore: ${parentLabel}`);
  const report = meta.lastReport ? truncateReport(meta.lastReport) : "";
  if (report) lines.push(`Ultimo report: ${report}`);
  return lines.join("\n");
}

/** Campi di `automation-update` / `AutomationTabInfo` (snake_case come da serde). */
export interface AutomationUpdateLike {
  status: string;
  title: string;
  parent_tab_id?: string | null;
  last_report?: string | null;
}

/**
 * Applica un aggiornamento ai metadati. Il report assente non cancella
 * l'ultimo noto; un titolo vuoto lascia quello attuale.
 */
export function applyAutomationUpdate(
  meta: AutomationMeta,
  u: AutomationUpdateLike,
): AutomationMeta {
  const title = u.title.trim() !== "" ? u.title : meta.title;
  const lastReport = u.last_report ? u.last_report : meta.lastReport;
  const parentTabId = u.parent_tab_id ? u.parent_tab_id : meta.parentTabId;
  if (
    title === meta.title &&
    lastReport === meta.lastReport &&
    parentTabId === meta.parentTabId &&
    u.status === meta.status
  ) {
    return meta;
  }
  return { ...meta, title, lastReport, parentTabId, status: u.status };
}

/** Tab da salvare nella sessione: gli automatici non si persistono ne' si ripristinano. */
export function persistableTabs<T extends { automation?: unknown }>(tabs: T[]): T[] {
  return tabs.filter((t) => !isAutomated(t));
}
