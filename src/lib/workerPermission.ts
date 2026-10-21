// Permessi dei worker automatici (per workspace). File a zero dipendenze:
// importabile sia dall'app sia dai test `node --test`. Rispecchia
// `PermissionMode` di src-tauri/src/agents/mod.rs.

export type WorkerPermission = "plan" | "default" | "accept-edits" | "bypass";

/** Dal piu' restrittivo al piu' permissivo. */
export const WORKER_PERMISSIONS: readonly WorkerPermission[] = [
  "plan",
  "default",
  "accept-edits",
  "bypass",
];

export const DEFAULT_WORKER_PERMISSION: WorkerPermission = "accept-edits";

const LABELS: Record<WorkerPermission, string> = {
  plan: "Sola lettura (plan)",
  default: "Chiedi per ogni azione",
  "accept-edits": "Approva modifiche ai file",
  bypass: "Nessuna conferma (bypass)",
};

export function workerPermissionLabel(mode: WorkerPermission): string {
  return LABELS[mode];
}

/** Valore sconosciuto (es. dati vecchi) = default, mai piu' permissivo. */
export function normalizeWorkerPermission(v: unknown): WorkerPermission {
  return WORKER_PERMISSIONS.find((m) => m === v) ?? DEFAULT_WORKER_PERMISSION;
}

/** Posizione nell'ordine di permissivita' (0 = plan). */
export function workerPermissionRank(mode: WorkerPermission): number {
  return WORKER_PERMISSIONS.indexOf(mode);
}

// --- Comandi pre-approvati (regole di permesso di Claude Code) ---
// Rispecchia `worker_rules::validate_rules` in src-tauri.

export const WORKER_RULES_MAX = 100;
export const WORKER_RULE_MAX_CHARS = 300;

/** Forma `Nome` o `Nome(...)`, Nome = [A-Za-z0-9_*-]+ ; `null` se valida. */
export function workerRuleError(rule: string): string | null {
  if (rule.length > WORKER_RULE_MAX_CHARS) return `più lunga di ${WORKER_RULE_MAX_CHARS} caratteri.`;
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f-\u009f]/.test(rule)) return "contiene caratteri di controllo.";
  const m = /^([A-Za-z0-9_*-]+)(\(.+\))?$/.exec(rule);
  if (!m) return "forma attesa: Nome oppure Nome(...), es. Bash(git:*).";
  if (m[2] && m[1].startsWith("mcp__")) return "le regole mcp__ non accettano parentesi.";
  return null;
}

/** Una regola per riga -> lista normalizzata (trim, righe vuote e doppioni tolti). */
export function parseWorkerRules(text: string): { rules: string[]; error: string | null } {
  const rules: string[] = [];
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const rule = lines[i].trim();
    if (!rule) continue;
    const err = workerRuleError(rule);
    if (err) return { rules, error: `Riga ${i + 1}: ${err}` };
    if (!rules.includes(rule)) rules.push(rule);
  }
  if (rules.length > WORKER_RULES_MAX) {
    return { rules, error: `Troppe regole (massimo ${WORKER_RULES_MAX}).` };
  }
  return { rules, error: null };
}

export const rulesToText = (rules: readonly string[] | undefined): string => (rules ?? []).join("\n");
