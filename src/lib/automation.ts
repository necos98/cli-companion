// Sessioni automatiche (worker avviati dallo scheduler dei task): metadati del
// tab, etichette di stato, tooltip ed esclusione dalla persistenza. Funzioni
// pure a zero dipendenze, importabili sia dall'app sia dai test `node --test`
// (type-stripping). Il backend e' src-tauri/src/automation.rs.

/** Stati di un worker (vedi `WorkerStatus` in automation/state.rs). */
export type WorkerStatus =
  | "starting"
  | "running"
  | "background"
  | "needs_input"
  | "idle"
  | "completed"
  | "failed"
  | "closed";

/** Metadati di un tab automatico (worker di un task). `status` e' una stringa: il backend puo' aggiungerne. */
export interface AutomationMeta {
  /** Titolo del task. */
  title: string;
  status: string;
  lastReport?: string;
  agentId: string;
  cwd?: string;
}

/** Id dell'avvio integrato "Task Intake" (preset, vedi agents/mod.rs). */
export const INTAKE_LAUNCH_ID = "builtin:preset:task-intake";

/** Badge dell'intestazione di un tab. */
export interface TabBadge {
  text: string;
  title: string;
  /** "intake" = tab del preset Task Intake (colore primary), "auto" = worker. */
  kind: "auto" | "intake";
}

/** Il tab e' stato aperto dall'avvio Task Intake? */
export function isIntakeLaunch(launchId: string | null | undefined): boolean {
  return launchId === INTAKE_LAUNCH_ID;
}

/**
 * Badge dell'intestazione di un tab: AUTO per i worker, INTAKE per i tab avviati
 * dal preset Task Intake, nessuno per gli altri.
 */
export function tabBadge(tab: {
  automation?: unknown;
  launchId?: string;
}): TabBadge | null {
  if (tab.automation) return { text: "AUTO", title: "Sessione automatica", kind: "auto" };
  if (isIntakeLaunch(tab.launchId)) {
    return { text: "INTAKE", title: "Task Intake: propone task per la coda", kind: "intake" };
  }
  return null;
}

/**
 * Chiave del tab Task Intake ancora aperto di un workspace (il primo in ordine
 * di creazione), o null se non ce n'e'.
 */
export function findIntakeTabKey(
  tabs: { key: string; workspace: { id: string }; launchId?: string }[],
  workspaceId: string,
): string | null {
  return (
    tabs.find((t) => t.workspace.id === workspaceId && isIntakeLaunch(t.launchId))?.key ?? null
  );
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

/** Testo del tooltip dell'intestazione: titolo del task, stato e ultimo report troncato. */
export function automationTooltip(meta: AutomationMeta): string {
  const lines = [`Sessione automatica: ${meta.title}`, `Stato: ${statusWord(meta.status).text}`];
  const report = meta.lastReport ? truncateReport(meta.lastReport) : "";
  if (report) lines.push(`Ultimo report: ${report}`);
  return lines.join("\n");
}

/** Campi di `automation-update` / `AutomationTabInfo` (snake_case come da serde). */
export interface AutomationUpdateLike {
  status: string;
  title: string;
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
  if (title === meta.title && lastReport === meta.lastReport && u.status === meta.status) {
    return meta;
  }
  return { ...meta, title, lastReport, status: u.status };
}

/** Tab da salvare nella sessione: gli automatici non si persistono ne' si ripristinano. */
export function persistableTabs<T extends { automation?: unknown }>(tabs: T[]): T[] {
  return tabs.filter((t) => !isAutomated(t));
}
