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
