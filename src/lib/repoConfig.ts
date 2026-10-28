// Impostazioni per repository (setup e pulizia dei worktree, test E2E): bozze del form,
// conversioni testo <-> elenco e validazione. Specchio di `RepoConfig::normalized`
// in src-tauri/src/workspaces.rs, che resta la fonte di verita'.

import type { E2eConfig, RepoConfig } from "./tauri";

export const MAX_REPO_COMMANDS = 30;
export const MAX_REPO_COMMAND_LEN = 2000;
export const MAX_REPO_PATH_LEN = 300;
export const DEFAULT_E2E_TIMEOUT_MIN = 20;
export const MIN_E2E_TIMEOUT_MIN = 1;
export const MAX_E2E_TIMEOUT_MIN = 60;

/** Controlli (tab escluso): il comando E2E sta su una riga. */
const CONTROL_CHARS = /[\u0000-\u0008\u000a-\u001f\u007f]/;

/** Bozza di un repository: i campi di testo come nel form. */
export interface RepoDraft {
  base_branch: string;
  container_root: string;
  copy_ignored: string;
  setup: string;
  cleanup: string;
  remove_force: boolean;
  e2e_command: string;
  e2e_junit: string;
  /** Minuti, come testo del campo; vuoto = predefinito. */
  e2e_timeout: string;
}

/** Righe non vuote (trim) di un testo. */
export function textToLines(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l !== "");
}

export function emptyDraft(): RepoDraft {
  return { base_branch: "", container_root: "", copy_ignored: "", setup: "", cleanup: "", remove_force: false, e2e_command: "", e2e_junit: "", e2e_timeout: "" };
}

export function draftFromConfig(cfg: RepoConfig | null | undefined): RepoDraft {
  if (!cfg) return emptyDraft();
  return {
    base_branch: cfg.base_branch ?? "",
    container_root: cfg.container_root ?? "",
    copy_ignored: (cfg.copy_ignored ?? []).join("\n"),
    setup: (cfg.setup ?? []).join("\n"),
    cleanup: (cfg.cleanup ?? []).join("\n"),
    remove_force: !!cfg.remove_force,
    e2e_command: cfg.e2e?.command ?? "",
    e2e_junit: cfg.e2e?.junit ?? "",
    e2e_timeout: cfg.e2e ? String(cfg.e2e.timeout_min) : "",
  };
}

/** Come il backend: stesse regole dei nomi di branch (`valid_branch` in board.rs). */
function validBranch(s: string): boolean {
  return (
    s.length > 0 &&
    s.length <= 200 &&
    !s.startsWith("-") &&
    !s.startsWith("/") &&
    !s.endsWith("/") &&
    !s.endsWith(".") &&
    !s.includes("..") &&
    !s.includes("//") &&
    /^[A-Za-z0-9._\-/]+$/.test(s)
  );
}

/** Percorso relativo al repository normalizzato (`/`, senza `./`), o l'errore; `label` apre i messaggi. */
function normalizeRelativePath(raw: string, label: string): { path: string } | { error: string } {
  const t = raw.trim().replace(/\\/g, "/");
  if (t.startsWith("/") || /^[A-Za-z]:/.test(t)) {
    return { error: `Il ${label} deve essere relativo al repository: ${t}` };
  }
  if (t.length > MAX_REPO_PATH_LEN) return { error: `${capitalize(label)} troppo lungo: ${t.slice(0, 40)}…` };
  const segs = t.split("/").filter((s) => s !== "" && s !== ".");
  if (segs.length === 0 || segs.includes("..")) {
    return { error: `${capitalize(label)} non valido (niente ".." né la radice): ${t}` };
  }
  return { path: segs.join("/") };
}

function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function commandsError(list: string[], what: string): string | null {
  if (list.length > MAX_REPO_COMMANDS) return `Troppi comandi di ${what} (max ${MAX_REPO_COMMANDS}).`;
  const long = list.find((c) => c.length > MAX_REPO_COMMAND_LEN);
  if (long) return `Comando di ${what} troppo lungo (max ${MAX_REPO_COMMAND_LEN} caratteri).`;
  return null;
}

/** Configurazione E2E dalla bozza: senza comando né percorso JUnit non c'e' (null); altrimenti serve tutto. */
function e2eFromDraft(d: RepoDraft): { e2e: E2eConfig | null } | { error: string } {
  const command = d.e2e_command.trim();
  const junit = d.e2e_junit.trim();
  const timeoutText = d.e2e_timeout.trim();
  if (!command && !junit && !timeoutText) return { e2e: null };
  if (!command) return { error: "Comando E2E mancante." };
  if (command.length > MAX_REPO_COMMAND_LEN) {
    return { error: `Comando E2E troppo lungo (max ${MAX_REPO_COMMAND_LEN} caratteri).` };
  }
  if (CONTROL_CHARS.test(command)) return { error: "Il comando E2E deve stare su una riga." };
  if (!junit) return { error: "Percorso JUnit E2E mancante." };
  const path = normalizeRelativePath(junit, "percorso JUnit");
  if ("error" in path) return path;
  const timeout = timeoutText === "" ? DEFAULT_E2E_TIMEOUT_MIN : Number(timeoutText);
  if (!Number.isInteger(timeout) || timeout < MIN_E2E_TIMEOUT_MIN || timeout > MAX_E2E_TIMEOUT_MIN) {
    return { error: `Timeout E2E non valido (da ${MIN_E2E_TIMEOUT_MIN} a ${MAX_E2E_TIMEOUT_MIN} minuti).` };
  }
  return { e2e: { command, junit: path.path, timeout_min: timeout } };
}

/** Configurazione da inviare al backend, oppure il primo errore di validazione. */
export function configFromDraft(d: RepoDraft): { config: RepoConfig; error: null } | { config: null; error: string } {
  const base = d.base_branch.trim();
  if (base && !validBranch(base)) {
    return { config: null, error: "Branch base non valido (lettere, cifre, . _ - /; niente spazi né \"..\")." };
  }
  const container = d.container_root.trim();
  const copy: string[] = [];
  for (const line of textToLines(d.copy_ignored)) {
    const r = normalizeRelativePath(line, "percorso");
    if ("error" in r) return { config: null, error: r.error };
    if (!copy.includes(r.path)) copy.push(r.path);
  }
  if (copy.length > MAX_REPO_COMMANDS) {
    return { config: null, error: `Troppi percorsi da copiare (max ${MAX_REPO_COMMANDS}).` };
  }
  const setup = textToLines(d.setup);
  const cleanup = textToLines(d.cleanup);
  const err = commandsError(setup, "setup") ?? commandsError(cleanup, "pulizia");
  if (err) return { config: null, error: err };
  const e2e = e2eFromDraft(d);
  if ("error" in e2e) return { config: null, error: e2e.error };
  return {
    error: null,
    config: {
      base_branch: base || null,
      container_root: container ? container.replace(/\/+$/, "") : null,
      copy_ignored: copy,
      setup,
      cleanup,
      remove_force: d.remove_force,
      ...(e2e.e2e ? { e2e: e2e.e2e } : {}),
    },
  };
}

/** La bozza coincide con la configurazione salvata? (nessuna salvata = bozza vuota) */
export function draftIsSaved(d: RepoDraft, saved: RepoConfig | null | undefined): boolean {
  const r = configFromDraft(d);
  if (r.config === null) return false;
  const s = saved ?? configFromDraft(emptyDraft()).config!;
  return JSON.stringify(r.config) === JSON.stringify(s);
}

export interface RepoRow {
  repo: string;
  /** Configurato ma non rilevato nel workspace ("non trovato"). */
  missing: boolean;
}

/** Repository rilevati più quelli configurati ma non rilevati. Senza rilevamento (null) nessuno e' "non trovato". */
export function repoRows(detected: string[] | null, configured: string[]): RepoRow[] {
  const rows: RepoRow[] = (detected ?? []).map((repo) => ({ repo, missing: false }));
  for (const repo of configured) {
    if (!rows.some((r) => r.repo === repo)) rows.push({ repo, missing: detected !== null });
  }
  return rows;
}
