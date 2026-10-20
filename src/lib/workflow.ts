// Orchestratore automatico e workflow per workspace (docs/SCHEDULE.MD): tipi,
// validazione e disponibilita' del wizard. Funzioni pure a zero dipendenze,
// importabili sia dall'app sia dai test `node --test`.

import { CLAUDE_AGENT_ID } from "./session.ts";

/** Configurazione `auto_orchestrate` di un workspace (serde snake_case). */
export interface AutoOrchestrate {
  enabled: boolean;
  agent: string;
  model: string | null;
}

/** Lunghezza massima del workflow (caratteri). */
export const WORKFLOW_MAX = 50000;

/** Lunghezza massima del nome di un modello. */
export const MODEL_MAX = 64;

/** Come `valid_model` del backend: lettere, cifre, '.', '_', '-' (1-64 caratteri). */
const MODEL_RE = /^[A-Za-z0-9._-]{1,64}$/;

/** Comando che avvia il wizard: Claude Code con la skill del plugin come prompt. */
export const WORKFLOW_WIZARD_COMMAND = 'claude "/cli-companion-notify:workflow"';

/** Titolo del tab del wizard. */
export const WORKFLOW_WIZARD_TITLE = "Wizard workflow";

/** Modello normalizzato: trim, nessuno spazio interno ammesso, `null` se vuoto. */
export function sanitizeModel(raw: string): string | null {
  const m = raw.trim().slice(0, MODEL_MAX);
  return m === "" ? null : m;
}

/** Errore sul nome del modello o null. */
export function modelError(raw: string): string | null {
  const m = raw.trim();
  if (m === "") return null;
  return MODEL_RE.test(m)
    ? null
    : `Il modello deve avere 1-${MODEL_MAX} caratteri tra lettere, cifre, '.', '_' e '-'`;
}

/** Configurazione pronta da inviare a `set_auto_orchestrate`. */
export function buildAutoOrchestrate(draft: {
  enabled: boolean;
  agent: string;
  model: string;
}): AutoOrchestrate {
  return { enabled: draft.enabled, agent: draft.agent, model: sanitizeModel(draft.model) };
}

/** Configurazione effettiva di un workspace (default: spenta, Claude Code). */
export function effectiveAutoOrchestrate(
  ws: { auto_orchestrate?: AutoOrchestrate | null },
  defaultAgent: string = CLAUDE_AGENT_ID,
): AutoOrchestrate {
  const a = ws.auto_orchestrate;
  return {
    enabled: a?.enabled === true,
    agent: a?.agent || defaultAgent,
    model: a?.model ?? null,
  };
}

/** Il workspace e' automatico (orchestratore automatico attivo)? */
export function isAutoWorkspace(ws: { auto_orchestrate?: AutoOrchestrate | null }): boolean {
  return ws.auto_orchestrate?.enabled === true;
}

/** Due configurazioni equivalenti? */
export function sameAutoOrchestrate(a: AutoOrchestrate, b: AutoOrchestrate): boolean {
  return a.enabled === b.enabled && a.agent === b.agent && (a.model ?? null) === (b.model ?? null);
}

/** Stato del workflow per l'etichetta. */
export function workflowStatusText(content: string | null | undefined): string {
  if (content === null || content === undefined || content.trim() === "") return "Assente";
  return `Presente · ${content.length} caratteri`;
}

/** Il testo e' salvabile? (non vuoto, entro il massimo) */
export function canSaveWorkflow(draft: string, saved: string | null): boolean {
  if (draft.trim() === "" || draft.length > WORKFLOW_MAX) return false;
  return draft !== (saved ?? "");
}

/** Campi di `AgentEnvStatus` usati dal controllo del wizard. */
export interface WizardEnvLike {
  agent_id: string;
  env_key: string;
  env_label: string;
  found: boolean;
  installed: boolean;
}

/**
 * Il wizard richiede Claude Code con il plugin installato nell'ambiente del
 * workspace (Windows oppure la sua distro WSL). `envs` vuoto = verifica ancora
 * in corso.
 */
export function wizardAvailability(
  ws: { kind: string; distro?: string | null },
  envs: WizardEnvLike[],
): { available: boolean; hint: string | null } {
  if (envs.length === 0) {
    return { available: false, hint: "Verifica dell'installazione di Claude Code in corso…" };
  }
  const key = ws.kind === "wsl" ? (ws.distro ?? "") : "windows";
  const env = envs.find((e) => e.agent_id === CLAUDE_AGENT_ID && e.env_key === key);
  if (!env || !env.found) {
    return {
      available: false,
      hint: `Claude Code non e' stato trovato in ${env?.env_label ?? (key || "questo ambiente")}`,
    };
  }
  if (!env.installed) {
    return {
      available: false,
      hint: `Installa il plugin di Claude Code per ${env.env_label} da Impostazioni › Coding agent`,
    };
  }
  return { available: true, hint: null };
}
