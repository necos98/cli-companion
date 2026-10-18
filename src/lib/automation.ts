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
export interface AutomationMeta {
  /** Tab dell'orchestratore che ha avviato il worker. */
  parentTabId: string;
  /** Titolo del task. */
  title: string;
  status: string;
  lastReport?: string;
  agentId: string;
  cwd?: string;
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
