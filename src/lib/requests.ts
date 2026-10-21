// Richieste dell'orchestratore all'utente (domande e proposte di impostazioni):
// tipi del contratto e helper puri (diff, etichette, ordinamento). Zero dipendenze.

export type RequestKind = "question" | "settings";
export type RequestStatus = "pending" | "answered" | "approved" | "rejected" | "dismissed";

export interface RepoSettingsPatch {
  base_branch?: string | null;
  container_root?: string | null;
  copy_ignored?: string[];
  setup?: string[];
  cleanup?: string[];
  remove_force?: boolean;
}

export interface SettingsPatch {
  workspace_rules?: string;
  /** Chiave = cartella del repo relativa alla root ("." = root). */
  repos?: Record<string, RepoSettingsPatch>;
}

export interface UserRequest {
  id: string;
  kind: RequestKind;
  status: RequestStatus;
  /** Unix ms. */
  created_at: number;
  resolved_at?: number;
  task?: string;
  question?: string;
  options?: string[];
  reason?: string;
  proposal?: SettingsPatch;
  current?: SettingsPatch;
  answer?: string;
  comment?: string;
}

export interface UserRequestEvent {
  workspace_id: string;
  request_id: string;
  kind: RequestKind;
  title: string;
  body: string;
}

/** Richieste in sospeso della board (campo `pending_requests` o, in mancanza, conteggio). */
export function pendingCount(
  board: { requests?: UserRequest[]; pending_requests?: number } | null | undefined,
): number {
  if (!board) return 0;
  if (typeof board.pending_requests === "number") return board.pending_requests;
  return (board.requests ?? []).filter((r) => r.status === "pending").length;
}

/** In sospeso: dalla più vecchia (si risponde in ordine). Risolte: dalla più recente. */
export function splitRequests(requests: UserRequest[] | undefined): {
  pending: UserRequest[];
  resolved: UserRequest[];
} {
  const all = requests ?? [];
  return {
    pending: all.filter((r) => r.status === "pending").sort((a, b) => a.created_at - b.created_at),
    resolved: all
      .filter((r) => r.status !== "pending")
      .sort((a, b) => (b.resolved_at ?? b.created_at) - (a.resolved_at ?? a.created_at)),
  };
}

export const REPO_FIELD_LABELS = {
  setup: "Comandi di setup",
  cleanup: "Comandi di pulizia",
  copy_ignored: "File ignorati da copiare",
  base_branch: "Branch base",
  container_root: "Percorso nel container",
  remove_force: "Rimozione forzata",
} as const;

export type RepoField = keyof typeof REPO_FIELD_LABELS;

/** Ordine di visualizzazione dei campi. */
export const REPO_FIELD_ORDER: RepoField[] = [
  "setup",
  "cleanup",
  "copy_ignored",
  "base_branch",
  "container_root",
  "remove_force",
];

export function repoLabel(key: string): string {
  return key === "." ? "(root)" : key;
}

export function resolutionLabel(r: Pick<UserRequest, "status" | "kind">): string {
  switch (r.status) {
    case "answered":
      return "Risposta inviata";
    case "approved":
      return "Approvata";
    case "rejected":
      return "Rifiutata";
    case "dismissed":
      return "Ignorata";
    default:
      return r.kind === "question" ? "Domanda in attesa" : "Proposta in attesa";
  }
}

export interface ListDiffItem {
  value: string;
  mark: "same" | "added" | "removed";
}

/** Diff elemento per elemento di due liste: rimossi, poi invariati/aggiunti nell'ordine nuovo. */
export function diffLists(before: string[] | undefined, after: string[] | undefined): ListDiffItem[] {
  const b = before ?? [];
  const a = after ?? [];
  const aSet = new Set(a);
  const bSet = new Set(b);
  const out: ListDiffItem[] = [];
  for (const v of b) if (!aSet.has(v)) out.push({ value: v, mark: "removed" });
  for (const v of a) out.push({ value: v, mark: bSet.has(v) ? "same" : "added" });
  return out;
}

export interface LineDiffItem {
  text: string;
  mark: "same" | "added" | "removed";
}

/** Diff per righe (LCS) di due testi. */
export function diffLines(before: string | undefined, after: string | undefined): LineDiffItem[] {
  const a = (before ?? "") === "" ? [] : (before as string).split("\n");
  const b = (after ?? "") === "" ? [] : (after as string).split("\n");
  const n = a.length;
  const m = b.length;
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }
  const out: LineDiffItem[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ text: a[i], mark: "same" });
      i++;
      j++;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      out.push({ text: a[i++], mark: "removed" });
    } else {
      out.push({ text: b[j++], mark: "added" });
    }
  }
  while (i < n) out.push({ text: a[i++], mark: "removed" });
  while (j < m) out.push({ text: b[j++], mark: "added" });
  return out;
}

/** Valore scalare per la riga "vecchio → nuovo". */
export function formatScalar(v: string | boolean | null | undefined): string {
  if (v === null || v === undefined || v === "") return "(non impostato)";
  if (typeof v === "boolean") return v ? "sì" : "no";
  return v;
}

export interface FieldChange {
  field: RepoField;
  label: string;
  /** "list" = diff per elemento; "scalar" = vecchio → nuovo. */
  type: "list" | "scalar";
  items?: ListDiffItem[];
  before?: string;
  after?: string;
}

export interface RepoChange {
  repo: string;
  label: string;
  fields: FieldChange[];
}

/** Modifiche per repository, nell'ordine dei campi; solo i campi presenti nella proposta. */
export function repoChanges(proposal?: SettingsPatch, current?: SettingsPatch): RepoChange[] {
  const out: RepoChange[] = [];
  for (const [repo, patch] of Object.entries(proposal?.repos ?? {})) {
    const cur = current?.repos?.[repo] ?? {};
    const fields: FieldChange[] = [];
    for (const field of REPO_FIELD_ORDER) {
      if (!(field in patch)) continue;
      const label = REPO_FIELD_LABELS[field];
      if (field === "setup" || field === "cleanup" || field === "copy_ignored") {
        fields.push({ field, label, type: "list", items: diffLists(cur[field], patch[field]) });
      } else {
        fields.push({
          field,
          label,
          type: "scalar",
          before: formatScalar(cur[field]),
          after: formatScalar(patch[field]),
        });
      }
    }
    if (fields.length > 0) out.push({ repo, label: repoLabel(repo), fields });
  }
  return out;
}
