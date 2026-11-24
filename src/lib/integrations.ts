// Integrazioni con i coding agent: etichette, stato per ambiente, azioni
// disponibili, abilitate/predefinita e impatto della disabilitazione. Logica
// pura a zero dipendenze: importabile dall'app e dai test `node --test`.

import type { AgentEnvStatus } from "../bindings/AgentEnvStatus.ts";
import type { Capability } from "../bindings/Capability.ts";
import type { IntegrationEnv } from "../bindings/IntegrationEnv.ts";
import type { IntegrationInfo } from "../bindings/IntegrationInfo.ts";
import type { TaskStatus } from "../bindings/TaskStatus.ts";

// ---------------------------------------------------------------------------
// Capability
// ---------------------------------------------------------------------------

/** Etichetta leggibile di ogni contratto: la tabella è esaustiva, un contratto nuovo in Rust non compila. */
export const CAPABILITY_LABELS: Record<Capability, string> = {
  package: "Pacchetto",
  "hook-events": "Notifiche",
  "session-resume": "Ripresa sessioni",
  automation: "Worker",
  "worker-permissions": "Permessi",
  "plugin-skills": "Skill del plugin",
  "user-skills": "Skill",
  "usage-limits": "Limiti di utilizzo",
  "session-usage": "Token per sessione",
  "input-profile": "Profilo di input",
  models: "Modelli",
  effort: "Effort",
  "interactive-launch": "Avvio dei Bot",
  "proxy-routing": "Proxy Headroom",
};

const CAPABILITY_ORDER = Object.keys(CAPABILITY_LABELS) as Capability[];

/** Etichette dei contratti dichiarati, nell'ordine della tabella e senza doppioni. */
export function capabilityLabels(declared: readonly Capability[]): string[] {
  const set = new Set(declared);
  return CAPABILITY_ORDER.filter((c) => set.has(c)).map((c) => CAPABILITY_LABELS[c]);
}

/** `true` se almeno una delle integrazioni dichiara almeno uno dei contratti. */
export function declaresAny(integrations: readonly IntegrationInfo[], wanted: readonly Capability[]): boolean {
  return integrations.some((i) => i.capabilities.some((c) => wanted.includes(c)));
}

// ---------------------------------------------------------------------------
// Stato per ambiente
// ---------------------------------------------------------------------------

export type EnvState = "not-found" | "found" | "ready" | "outdated" | "installed";

/** Campi di `IntegrationEnv` usati per lo stato. */
export type EnvLike = Pick<IntegrationEnv, "readiness"> & {
  status: Pick<AgentEnvStatus, "found" | "installed" | "version">;
};

const ENV_STATE_TEXT: Record<EnvState, string> = {
  "not-found": "Non rilevato",
  found: "Trovato",
  ready: "Pronto",
  outdated: "Da aggiornare",
  installed: "Installato",
};

/**
 * Stato di un ambiente. Per un'integrazione disabilitata la prontezza non
 * rileva nulla: si legge lo stato grezzo dell'ambiente (`installed` = pacchetto
 * ancora presente).
 */
export function envState(env: EnvLike): EnvState {
  switch (env.readiness) {
    case "ready":
      return "ready";
    case "outdated":
      return "outdated";
    case "not-installed":
      return "found";
    case "not-found":
      return "not-found";
    case "disabled":
      if (!env.status.found) return "not-found";
      return env.status.installed ? "installed" : "found";
  }
}

/** Testo di stato, con la versione dell'agente se nota. */
export function envStateText(env: EnvLike): string {
  const state = envState(env);
  const version = env.status.version?.trim();
  const text = ENV_STATE_TEXT[state];
  return state !== "not-found" && version ? `${text} · v${version}` : text;
}

export type EnvAction = "install" | "update" | "remove";

export const ENV_ACTION_LABELS: Record<EnvAction, string> = {
  install: "Installa",
  update: "Aggiorna",
  remove: "Rimuovi",
};

/**
 * Azioni offerte su un ambiente. Un'integrazione disabilitata non ne ha:
 * non si installa né si aggiorna, il pacchetto si toglie da `removableEnvs`.
 */
export function envActions(env: EnvLike, integrationEnabled: boolean): EnvAction[] {
  if (!integrationEnabled) return [];
  switch (envState(env)) {
    case "found":
      return ["install"];
    case "outdated":
      return ["update", "remove"];
    case "ready":
      return ["remove"];
    case "installed":
    case "not-found":
      return [];
  }
}

/** Ambienti da cui "Rimuovi dagli ambienti" toglie il pacchetto: agente trovato e pacchetto presente. */
export function removableEnvs(info: Pick<IntegrationInfo, "envs">): IntegrationEnv[] {
  return info.envs.filter((e) => e.status.found && e.status.installed);
}

// ---------------------------------------------------------------------------
// Abilitate e predefinita
// ---------------------------------------------------------------------------

const enabledCache = new WeakMap<readonly IntegrationInfo[], IntegrationInfo[]>();
const envStatusCache = new WeakMap<readonly IntegrationInfo[], AgentEnvStatus[]>();

/** Integrazioni abilitate. Stesso riferimento finché l'elenco è lo stesso (selettore stabile per i componenti). */
export function enabledOf(integrations: readonly IntegrationInfo[]): IntegrationInfo[] {
  let enabled = enabledCache.get(integrations);
  if (!enabled) {
    enabled = integrations.filter((i) => i.enabled);
    enabledCache.set(integrations, enabled);
  }
  return enabled;
}

/** L'integrazione predefinita, se c'è ed è abilitata. */
export function defaultOf(integrations: readonly IntegrationInfo[]): IntegrationInfo | null {
  return enabledOf(integrations).find((i) => i.is_default) ?? null;
}

/** Stato degli ambienti delle sole integrazioni abilitate (barra di stato, utilizzo). */
export function enabledEnvStatuses(integrations: readonly IntegrationInfo[]): AgentEnvStatus[] {
  let statuses = envStatusCache.get(integrations);
  if (!statuses) {
    statuses = enabledOf(integrations).flatMap((i) => i.envs.map((e) => e.status));
    envStatusCache.set(integrations, statuses);
  }
  return statuses;
}

/** Elemento con un `agent_id` fra gli abilitati: filtra profili e simili. */
export function onlyEnabled<T extends { agent_id: string }>(
  items: readonly T[],
  integrations: readonly IntegrationInfo[],
): T[] {
  const ids = new Set(enabledOf(integrations).map((i) => i.id));
  return items.filter((item) => ids.has(item.agent_id));
}

// ---------------------------------------------------------------------------
// Impatto della disabilitazione
// ---------------------------------------------------------------------------

export interface DisableImpact {
  /** Terminali aperti in cui gira l'agente. */
  tabs: number;
  /** Task in coda che usano l'agente. */
  queuedTasks: number;
}

export interface TaskAgentLike {
  agent: string;
  status: TaskStatus;
}

/** Quanti terminali (`tabAgents`: agente di ogni tab, o null) e task in coda dipendono dall'agente. */
export function disableImpact(
  agentId: string,
  tabAgents: ReadonlyArray<string | null>,
  tasks: readonly TaskAgentLike[],
): DisableImpact {
  return {
    tabs: tabAgents.filter((a) => a === agentId).length,
    queuedTasks: tasks.filter((t) => t.agent === agentId && t.status === "queued").length,
  };
}

const plural = (n: number, one: string, many: string): string => (n === 1 ? `1 ${one}` : `${n} ${many}`);

/** Cosa succede disabilitando: righe da mostrare nella conferma. Vuoto se nulla dipende dall'agente. */
export function disableImpactLines(name: string, impact: DisableImpact): string[] {
  const lines: string[] = [];
  if (impact.tabs > 0) {
    lines.push(
      `${plural(impact.tabs, "terminale aperto", "terminali aperti")} con ${name}: restano aperti, ma l’integrazione non sarà più riconosciuta.`,
    );
  }
  if (impact.queuedTasks > 0) {
    lines.push(`${impact.queuedTasks} task in coda con ${name}: si fermano con un avviso finché non la riabiliti.`);
  }
  return lines;
}
