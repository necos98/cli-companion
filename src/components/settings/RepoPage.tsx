import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { ChevronDown, ChevronRight, Loader2 } from "lucide-react";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { api } from "../../lib/tauri";
import type { RepoConfig, Workspace } from "../../lib/tauri";
import {
  configFromDraft,
  draftFromConfig,
  draftIsSaved,
  repoRows,
} from "../../lib/repoConfig";
import type { RepoDraft } from "../../lib/repoConfig";
import { cn } from "../../lib/utils";
import { Field, Section, SettingsPage, useWorkspaceOp } from "./shared";

const areaClass =
  "w-full resize-y rounded-md border border-input bg-transparent px-2 py-1.5 font-mono text-[12px] outline-none placeholder:text-muted-foreground focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50";

const EMPTY: Record<string, RepoConfig> = {};

interface CardProps {
  workspaceId: string;
  repo: string;
  saved: RepoConfig | undefined;
  missing: boolean;
  onWorkspaceUpdated: (ws: Workspace) => void;
}

/** Scheda di un repository: si apre al bisogno, ha bozza e salvataggio propri. */
const RepoCard = memo(function RepoCard({ workspaceId, repo, saved, missing, onWorkspaceUpdated }: CardProps) {
  const { run, saving, error, setError } = useWorkspaceOp(onWorkspaceUpdated);
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<RepoDraft>(() => draftFromConfig(saved));
  const [validation, setValidation] = useState<string | null>(null);

  // La bozza segue la configurazione salvata (altro workspace, salvataggio): per
  // contenuto, cosi' un salvataggio altrove non azzera le modifiche di questa scheda.
  const savedKey = JSON.stringify(saved ?? null);
  useEffect(() => {
    setDraft(draftFromConfig(saved));
    setValidation(null);
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspaceId, repo, savedKey, setError]);

  const patch = (p: Partial<RepoDraft>) => {
    setDraft((d) => ({ ...d, ...p }));
    setValidation(null);
  };
  const unchanged = draftIsSaved(draft, saved);

  const save = () => {
    const r = configFromDraft(draft);
    if (r.config === null) {
      setValidation(r.error);
      return;
    }
    void run(() => api.setRepoConfig(workspaceId, repo, r.config));
  };

  return (
    <Section className="gap-2">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="flex w-full items-center gap-1.5 text-left"
      >
        {open ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
        <span className="font-mono text-[12px] font-semibold">{repo}</span>
        {missing && <span className="rounded border px-1 py-px text-[10px] text-destructive">non trovato</span>}
        {saved && <span className="rounded border px-1 py-px text-[10px] text-muted-foreground">configurato</span>}
      </button>
      {open && (
        <div className="flex flex-col gap-3 pt-1">
          <div className="grid gap-3 md:grid-cols-2">
            <Field label="Branch base">
              <Input
                value={draft.base_branch}
                onChange={(e) => patch({ base_branch: e.target.value })}
                placeholder="automatico (origin/HEAD)"
                className="h-8 font-mono text-[12px]"
              />
            </Field>
            <Field label="Percorso nel container">
              <Input
                value={draft.container_root}
                onChange={(e) => patch({ container_root: e.target.value })}
                placeholder="/var/www/html"
                className="h-8 font-mono text-[12px]"
              />
            </Field>
          </div>
          <Field
            label="File ignorati da copiare"
            help="Uno per riga, relativi al repository (es. configuration.php, vendor). Si copiano nel worktree nuovo solo se git li ignora e esistono; gli altri vengono saltati con un avviso."
          >
            <textarea
              value={draft.copy_ignored}
              onChange={(e) => patch({ copy_ignored: e.target.value })}
              rows={3}
              spellCheck={false}
              placeholder={"configuration.php\npublic/api/.env"}
              className={areaClass}
            />
          </Field>
          <Field
            label="Comandi di setup"
            help="Uno per riga, in ordine, nel worktree appena creato. Se uno fallisce il task non parte (e al retry il setup si rifà): scrivili ripetibili."
          >
            <textarea
              value={draft.setup}
              onChange={(e) => patch({ setup: e.target.value })}
              rows={3}
              spellCheck={false}
              placeholder={'docker exec -w "$CONTAINER_WORKTREE" svc composer dump-autoload'}
              className={areaClass}
            />
          </Field>
          <Field
            label="Comandi di pulizia"
            help="Uno per riga, nel worktree, prima di rimuoverlo quando il task passa a done. Un errore è solo un avviso."
          >
            <textarea
              value={draft.cleanup}
              onChange={(e) => patch({ cleanup: e.target.value })}
              rows={2}
              spellCheck={false}
              placeholder={'docker exec svc rm -rf "$CONTAINER_WORKTREE/cache"'}
              className={areaClass}
            />
          </Field>
          <label className="flex items-center gap-2 text-[12px]">
            <input
              type="checkbox"
              checked={draft.remove_force}
              onChange={(e) => patch({ remove_force: e.target.checked })}
            />
            Rimuovi il worktree anche se sporco (--force)
          </label>
          {(validation ?? error) && (
            <p className={cn("text-[11px] text-destructive")}>{validation ?? error}</p>
          )}
          <div className="flex gap-2">
            <Button size="sm" variant="secondary" disabled={saving || unchanged} onClick={save}>
              Salva
            </Button>
            {saved && (
              <Button
                size="sm"
                variant="ghost"
                disabled={saving}
                onClick={() => void run(() => api.setRepoConfig(workspaceId, repo, null))}
              >
                Rimuovi configurazione
              </Button>
            )}
          </div>
        </div>
      )}
    </Section>
  );
});

interface Props {
  workspace: Workspace;
  onWorkspaceUpdated: (ws: Workspace) => void;
}

/** Workspace › Repository: setup e pulizia dei worktree, eseguiti dall'app, per ogni repository. */
export default function RepoPage({ workspace, onWorkspaceUpdated }: Props) {
  const [detected, setDetected] = useState<string[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [detectError, setDetectError] = useState<string | null>(null);

  // Rilevamento in background: la pagina si apre subito.
  useEffect(() => {
    let cancelled = false;
    setDetected(null);
    setDetectError(null);
    setLoading(true);
    api
      .workspaceDetectRepos(workspace.id)
      .then((r) => !cancelled && setDetected(r))
      .catch((e) => !cancelled && setDetectError(String(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [workspace.id]);

  const repos = workspace.repos ?? EMPTY;
  const rows = useMemo(() => repoRows(detected, Object.keys(repos)), [detected, repos]);
  const updated = useCallback((ws: Workspace) => onWorkspaceUpdated(ws), [onWorkspaceUpdated]);

  return (
    <SettingsPage
      title="Repository"
      description="Preparazione e pulizia dei worktree per ogni repository del workspace, eseguite dall’app."
    >
      <Section>
        <div className="text-[11px] text-muted-foreground">
          <p>
            I comandi girano nel worktree, con <span className="font-mono">sh -c</span> nei workspace WSL e{" "}
            <span className="font-mono">cmd /C</span> su Windows. Variabili disponibili:{" "}
            <span className="font-mono">$REPO</span> (cartella del repository),{" "}
            <span className="font-mono">$WORKTREE</span>, <span className="font-mono">$CONTAINER_WORKTREE</span>{" "}
            (percorso nel container, vuoto senza «Percorso nel container»),{" "}
            <span className="font-mono">$TASK_ID</span> (<span className="font-mono">integration</span> per il
            worktree di integrazione), <span className="font-mono">$BRANCH</span>,{" "}
            <span className="font-mono">$BASE</span>. Su Windows si scrivono <span className="font-mono">%REPO%</span>{" "}
            ecc.
          </p>
          <p className="mt-1">
            Il setup parte solo sui worktree creati dall’app (task e integrazione); un worktree riusato dopo un
            retry non lo rifà, a meno che il setup precedente non fosse finito male. Ogni comando ha 10 minuti. Il
            branch base vale dopo quelli della board e prima di origin/HEAD.
          </p>
        </div>
      </Section>
      {loading && (
        <p className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
          <Loader2 className="h-3.5 w-3.5 animate-spin" /> Ricerca dei repository…
        </p>
      )}
      {detectError && <p className="text-[11px] text-destructive">Rilevamento non riuscito: {detectError}</p>}
      {!loading && !detectError && rows.length === 0 && (
        <p className="text-[11px] text-muted-foreground">Nessun repository git trovato nel workspace.</p>
      )}
      {rows.map((r) => (
        <RepoCard
          key={`${workspace.id}:${r.repo}`}
          workspaceId={workspace.id}
          repo={r.repo}
          saved={repos[r.repo]}
          missing={r.missing}
          onWorkspaceUpdated={updated}
        />
      ))}
    </SettingsPage>
  );
}
