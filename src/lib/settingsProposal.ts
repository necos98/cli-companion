// Proposte di modifica alle impostazioni del workspace: trasforma `patch` +
// `current` + `sensitive` di una `SettingsProposalView` nelle righe del
// confronto "prima → dopo" mostrato dalla card. Funzioni pure: importabili sia
// dall'app sia dai test `node --test`. Le etichette sono quelle dei form delle
// impostazioni, così i nomi coincidono con la UI.

import { deliveryLabel, mergeMethodLabel } from "./board.ts";
import type { RepoPatch, SettingsPatch, SettingsProposalView } from "./board.ts";
import type { E2eConfig } from "./tauri.ts";
import { SHELL_LAUNCH_ID } from "./launch.ts";
import { WORKER_PERMISSIONS, workerPermissionLabel } from "./workerPermission.ts";
import type { WorkerPermission } from "./workerPermission.ts";

/** Chiave del repository che coincide con la root del workspace. */
export const ROOT_REPO_KEY = ".";

/** Testi dei valori assenti. */
export const EMPTY_TEXT = "(vuoto)";
export const UNKNOWN_TEXT = "—";
export const MODEL_DEFAULT_TEXT = "Predefinito dell’agente";
export const LAUNCH_INHERIT_TEXT = "Eredita dalle impostazioni generali";
export const LAUNCH_SHELL_TEXT = "Shell vuota";
export const BRANCH_DEFAULT_TEXT = "Predefinita";
export const CONTAINER_ROOT_NONE_TEXT = "Nessuno";
export const FLAG_ON_TEXT = "Acceso";
export const FLAG_OFF_TEXT = "Spento";
export const REMOVE_REPO_TEXT = "Rimuovi configurazione";
export const E2E_NONE_TEXT = "Nessuno";

/** Note dei campi sensibili. */
export const NOTE_RUNS_COMMANDS = "esegue comandi sul tuo PC";
export const NOTE_WIDENS_PERMISSIONS = "allarga i permessi dei worker";

/** Un valore più lungo di così (o su più righe) si mostra compresso. */
export const LONG_TEXT_MIN = 120;

/** Etichette dei campi. */
const LABEL = {
  worker_rules: "Regole per i worker",
  worker_model: "Modello dei worker",
  worker_permission: "Permessi dei worker automatici",
  worker_allow: "Comandi consentiti",
  worker_deny: "Comandi vietati",
  mode: "Modalità di consegna",
  parallel: "Worker in parallelo (worktree)",
  merge_method: "Metodo di merge",
  use_schedule: "Rispetta le fasce orarie",
  default_launch: "Avvio predefinito",
  launches: "Avvii",
  base_branch: "Branch base",
  container_root: "Percorso nel container",
  copy_ignored: "File ignorati da copiare",
  setup: "Comandi di setup",
  cleanup: "Comandi di pulizia",
  remove_force: "Rimuovi il worktree anche se sporco (--force)",
  e2e: "Test E2E",
} as const;

/** Valore singolo: prima e dopo come testo (`before` null = non noto). */
export interface ValueRow {
  kind: "value";
  field: string;
  group: string | null;
  label: string;
  before: string | null;
  after: string;
  /** Testo lungo o su più righe: la card lo mostra comprimibile. */
  long: boolean;
  /** Nota del campo sensibile, se lo è. */
  sensitive: string | null;
}

/** Elenco: voci tolte e voci aggiunte (l'ordine non conta). */
export interface ListRow {
  kind: "list";
  field: string;
  group: string | null;
  label: string;
  removed: string[];
  added: string[];
  sensitive: string | null;
}

/** Rimozione della configurazione di un repository. */
export interface RemoveRow {
  kind: "remove";
  field: string;
  group: string | null;
  label: string;
  sensitive: null;
}

export type SettingsRow = ValueRow | ListRow | RemoveRow;

/** Nota di un campo sensibile (nome dato dal backend), `null` se non lo è. */
export function sensitiveNote(field: string, sensitive: readonly string[]): string | null {
  if (!sensitive.includes(field)) return null;
  const permissions = field === "worker_permission" || field === "worker_allow" || field === "worker_deny";
  return permissions ? NOTE_WIDENS_PERMISSIONS : NOTE_RUNS_COMMANDS;
}

/** Titolo del gruppo di un repository ("." = la root del workspace). */
export function repoGroupTitle(key: string): string {
  return key === ROOT_REPO_KEY ? "Repository: root del workspace" : `Repository: ${key}`;
}

function isLong(...texts: (string | null)[]): boolean {
  return texts.some((t) => t !== null && (t.length > LONG_TEXT_MIN || t.includes("\n")));
}

/** Voci di `before` assenti da `after`, nell'ordine di `before`. */
function missingFrom(before: readonly string[], after: readonly string[]): string[] {
  const keep = new Set(after);
  return before.filter((v) => !keep.has(v));
}

const textOrEmpty = (s: string | null | undefined, empty = EMPTY_TEXT): string => (s ? s : empty);
const flagText = (v: boolean): string => (v ? FLAG_ON_TEXT : FLAG_OFF_TEXT);

/** Comando, report e timeout di una configurazione E2E su una riga. */
export function e2eText(e2e: E2eConfig | null | undefined): string {
  if (!e2e) return E2E_NONE_TEXT;
  return `${e2e.command} (JUnit: ${e2e.junit}, timeout ${e2e.timeout_min} min)`;
}

function permissionText(v: string): string {
  return WORKER_PERMISSIONS.includes(v as WorkerPermission) ? workerPermissionLabel(v as WorkerPermission) : v;
}

function valueRow(
  ctx: Ctx,
  field: string,
  label: string,
  before: string | null,
  after: string,
  group: string | null = null,
): ValueRow {
  return {
    kind: "value",
    field,
    group,
    label,
    before,
    after,
    long: isLong(before, after),
    sensitive: sensitiveNote(field, ctx.sensitive),
  };
}

function listRow(
  ctx: Ctx,
  field: string,
  label: string,
  before: readonly string[] | undefined,
  after: readonly string[],
  group: string | null = null,
): ListRow {
  return {
    kind: "list",
    field,
    group,
    label,
    removed: missingFrom(before ?? [], after),
    added: missingFrom(after, before ?? []),
    sensitive: sensitiveNote(field, ctx.sensitive),
  };
}

interface Ctx {
  sensitive: readonly string[];
  /** Nome dell'avvio dato il suo id (da patch e valori attuali). */
  launchName: (id: string) => string;
}

// ---------------------------------------------------------------- avvii

type LaunchEntry = NonNullable<SettingsPatch["launches"]>[number];

const launchLine = (l: LaunchEntry): string => `${l.name}: ${l.command}`;

/**
 * Avvii proposti confrontati con gli attuali: per id se la voce lo ha, per
 * nome altrimenti. Una voce cambiata compare tolta (vecchia) e aggiunta
 * (nuova); gli avvii attuali non più presenti sono tolti.
 */
export function diffLaunches(
  current: readonly LaunchEntry[] | undefined,
  proposed: readonly LaunchEntry[],
): { removed: string[]; added: string[] } {
  const before = current ?? [];
  const matched = new Set<LaunchEntry>();
  const added: string[] = [];
  const removed: string[] = [];
  for (const next of proposed) {
    const prev = before.find(
      (c) => !matched.has(c) && (next.id !== undefined ? c.id === next.id : c.name === next.name),
    );
    if (!prev) {
      added.push(launchLine(next));
      continue;
    }
    matched.add(prev);
    if (prev.name !== next.name || prev.command !== next.command) {
      removed.push(launchLine(prev));
      added.push(launchLine(next));
    }
  }
  for (const prev of before) if (!matched.has(prev)) removed.push(launchLine(prev));
  return { removed, added };
}

function launchRow(ctx: Ctx, patch: SettingsPatch, current: SettingsPatch): ListRow | null {
  if (!patch.launches) return null;
  const { removed, added } = diffLaunches(current.launches, patch.launches);
  return {
    kind: "list",
    field: "launches",
    group: null,
    label: LABEL.launches,
    removed,
    added,
    sensitive: sensitiveNote("launches", ctx.sensitive),
  };
}

function launchChoiceText(ctx: Ctx, id: string | null | undefined): string | null {
  if (id === undefined) return null;
  if (id === null) return LAUNCH_INHERIT_TEXT;
  return id === SHELL_LAUNCH_ID ? LAUNCH_SHELL_TEXT : ctx.launchName(id);
}

// ---------------------------------------------------------------- campi del workspace

function workerRows(ctx: Ctx, patch: SettingsPatch, current: SettingsPatch): SettingsRow[] {
  const rows: SettingsRow[] = [];
  if (patch.worker_rules !== undefined) {
    const before = current.worker_rules === undefined ? null : textOrEmpty(current.worker_rules);
    rows.push(valueRow(ctx, "worker_rules", LABEL.worker_rules, before, textOrEmpty(patch.worker_rules)));
  }
  if (patch.worker_model !== undefined) {
    const before = current.worker_model === undefined ? null : textOrEmpty(current.worker_model, MODEL_DEFAULT_TEXT);
    rows.push(valueRow(ctx, "worker_model", LABEL.worker_model, before, textOrEmpty(patch.worker_model, MODEL_DEFAULT_TEXT)));
  }
  if (patch.worker_permission !== undefined) {
    const before = current.worker_permission === undefined ? null : permissionText(current.worker_permission);
    rows.push(valueRow(ctx, "worker_permission", LABEL.worker_permission, before, permissionText(patch.worker_permission)));
  }
  if (patch.worker_allow) rows.push(listRow(ctx, "worker_allow", LABEL.worker_allow, current.worker_allow, patch.worker_allow));
  if (patch.worker_deny) rows.push(listRow(ctx, "worker_deny", LABEL.worker_deny, current.worker_deny, patch.worker_deny));
  return rows;
}

function deliveryRows(ctx: Ctx, patch: SettingsPatch, current: SettingsPatch): SettingsRow[] {
  const next = patch.delivery;
  if (!next) return [];
  const now = current.delivery ?? {};
  const rows: SettingsRow[] = [];
  if (next.mode !== undefined) {
    rows.push(valueRow(ctx, "delivery.mode", LABEL.mode, now.mode ? deliveryLabel(now.mode) : null, deliveryLabel(next.mode)));
  }
  if (next.parallel !== undefined) {
    rows.push(valueRow(ctx, "delivery.parallel", LABEL.parallel, now.parallel === undefined ? null : flagText(now.parallel), flagText(next.parallel)));
  }
  if (next.merge_method !== undefined) {
    const before = now.merge_method ? mergeMethodLabel(now.merge_method) : null;
    rows.push(valueRow(ctx, "delivery.merge_method", LABEL.merge_method, before, mergeMethodLabel(next.merge_method)));
  }
  return rows;
}

function launchRows(ctx: Ctx, patch: SettingsPatch, current: SettingsPatch): SettingsRow[] {
  const rows: SettingsRow[] = [];
  if (patch.use_schedule !== undefined) {
    const before = current.use_schedule === undefined ? null : flagText(current.use_schedule);
    rows.push(valueRow(ctx, "use_schedule", LABEL.use_schedule, before, flagText(patch.use_schedule)));
  }
  const launches = launchRow(ctx, patch, current);
  if (launches) rows.push(launches);
  if (patch.default_launch !== undefined) {
    const after = launchChoiceText(ctx, patch.default_launch) ?? LAUNCH_INHERIT_TEXT;
    rows.push(valueRow(ctx, "default_launch", LABEL.default_launch, launchChoiceText(ctx, current.default_launch), after));
  }
  return rows;
}

// ---------------------------------------------------------------- repository

function repoRows(ctx: Ctx, key: string, patch: RepoPatch | null, current: RepoPatch | null | undefined): SettingsRow[] {
  const group = repoGroupTitle(key);
  const field = (name: string) => `repos[${key}].${name}`;
  if (patch === null) {
    return [{ kind: "remove", field: `repos[${key}]`, group, label: REMOVE_REPO_TEXT, sensitive: null }];
  }
  // `current` null = nessuna configurazione salvata (valori di default); undefined = workspace non noto.
  const known = current !== undefined;
  const now = current ?? {};
  const rows: SettingsRow[] = [];
  if (patch.base_branch !== undefined) {
    const before = known ? textOrEmpty(now.base_branch, BRANCH_DEFAULT_TEXT) : null;
    rows.push(valueRow(ctx, field("base_branch"), LABEL.base_branch, before, textOrEmpty(patch.base_branch, BRANCH_DEFAULT_TEXT), group));
  }
  if (patch.container_root !== undefined) {
    const before = known ? textOrEmpty(now.container_root, CONTAINER_ROOT_NONE_TEXT) : null;
    rows.push(valueRow(ctx, field("container_root"), LABEL.container_root, before, textOrEmpty(patch.container_root, CONTAINER_ROOT_NONE_TEXT), group));
  }
  if (patch.copy_ignored) rows.push(listRow(ctx, field("copy_ignored"), LABEL.copy_ignored, now.copy_ignored, patch.copy_ignored, group));
  if (patch.setup) rows.push(listRow(ctx, field("setup"), LABEL.setup, now.setup, patch.setup, group));
  if (patch.cleanup) rows.push(listRow(ctx, field("cleanup"), LABEL.cleanup, now.cleanup, patch.cleanup, group));
  if (patch.remove_force !== undefined) {
    const before = known ? flagText(now.remove_force ?? false) : null;
    rows.push(valueRow(ctx, field("remove_force"), LABEL.remove_force, before, flagText(patch.remove_force), group));
  }
  if (patch.e2e !== undefined) {
    const before = known ? e2eText(now.e2e) : null;
    rows.push(valueRow(ctx, field("e2e"), LABEL.e2e, before, e2eText(patch.e2e), group));
  }
  return rows;
}

function allRepoRows(ctx: Ctx, patch: SettingsPatch, current: SettingsPatch): SettingsRow[] {
  return Object.entries(patch.repos ?? {}).flatMap(([key, repo]) => repoRows(ctx, key, repo, current.repos?.[key]));
}

// ---------------------------------------------------------------- API

function launchNameLookup(patch: SettingsPatch, current: SettingsPatch): (id: string) => string {
  const names = new Map<string, string>();
  for (const l of [...(current.launches ?? []), ...(patch.launches ?? [])]) {
    if (l.id !== undefined) names.set(l.id, l.name);
  }
  return (id) => names.get(id) ?? id;
}

/**
 * Righe del confronto di una proposta, nell'ordine dei form: worker, consegna,
 * fasce orarie e avvii, poi un gruppo per repository (chiavi nell'ordine della
 * patch). Una riga è sensibile se il backend ne ha elencato il campo.
 */
export function settingsProposalRows(
  proposal: Pick<SettingsProposalView, "patch" | "current" | "sensitive">,
): SettingsRow[] {
  const { patch, sensitive } = proposal;
  const current = proposal.current ?? {};
  const ctx: Ctx = { sensitive, launchName: launchNameLookup(patch, current) };
  return [
    ...workerRows(ctx, patch, current),
    ...deliveryRows(ctx, patch, current),
    ...launchRows(ctx, patch, current),
    ...allRepoRows(ctx, patch, current),
  ];
}
