import { useEffect, useState } from "react";
import { Button } from "../ui/button";
import { api } from "../../lib/tauri";
import type { AgentEnvStatus, Workspace } from "../../lib/tauri";
import {
  WORKFLOW_MAX,
  canSaveWorkflow,
  wizardAvailability,
  workflowStatusText,
} from "../../lib/workflow";
import { Section, SettingsPage } from "./shared";

interface Props {
  workspace: Workspace;
  agentEnvs: AgentEnvStatus[];
  /** Chiude le impostazioni e apre il terminale del wizard nel workspace. */
  onCreateWorkflow: (ws: Workspace) => void;
}

/** Workspace › Workflow: editor del metodo di lavoro dell'orchestratore. */
export default function WorkflowPage({ workspace, agentEnvs, onCreateWorkflow }: Props) {
  // Testo salvato (null = assente) e bozza dell'editor.
  const [saved, setSaved] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setConfirmDelete(false);
    api
      .workflowGet(workspace.id)
      .then((w) => {
        if (cancelled) return;
        setSaved(w);
        setDraft(w ?? "");
      })
      .catch((e) => {
        if (cancelled) return;
        setSaved(null);
        setDraft("");
        setError(String(e));
      })
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [workspace.id]);

  const save = async () => {
    setError(null);
    setBusy(true);
    try {
      await api.workflowSave(workspace.id, draft);
      setSaved(draft);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setError(null);
    setBusy(true);
    try {
      await api.workflowSave(workspace.id, null);
      setSaved(null);
      setDraft("");
      setConfirmDelete(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const wizard = wizardAvailability(workspace, agentEnvs);

  return (
    <SettingsPage
      title="Workflow"
      description="Il metodo di lavoro che l’orchestratore di questo workspace segue."
    >
      <Section>
        <div className="flex items-center justify-between">
          <p className="text-xs font-semibold">Workflow</p>
          <span className="text-[11px] text-muted-foreground">
            {loading ? "Caricamento…" : workflowStatusText(saved)}
          </span>
        </div>
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          disabled={loading}
          maxLength={WORKFLOW_MAX}
          rows={12}
          spellCheck={false}
          placeholder="Metodo di lavoro dell’orchestratore di questo workspace (Markdown, in inglese)"
          className="w-full resize-y rounded-md border border-input bg-transparent px-3 py-2 font-mono text-[12px] leading-relaxed shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
        />
        <div className="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            disabled={busy || loading || !canSaveWorkflow(draft, saved)}
            onClick={() => void save()}
          >
            Salva
          </Button>
          {confirmDelete ? (
            <>
              <Button
                size="sm"
                variant="destructive"
                disabled={busy}
                onClick={() => void remove()}
              >
                Conferma l’eliminazione
              </Button>
              <Button size="sm" variant="secondary" onClick={() => setConfirmDelete(false)}>
                Annulla
              </Button>
            </>
          ) : (
            <Button
              size="sm"
              variant="secondary"
              disabled={busy || loading || saved === null}
              onClick={() => setConfirmDelete(true)}
            >
              Elimina
            </Button>
          )}
          <Button
            size="sm"
            variant="outline"
            className="ml-auto"
            disabled={!wizard.available}
            title={wizard.hint ?? "Apre Claude Code con la skill che ti intervista"}
            onClick={() => onCreateWorkflow(workspace)}
          >
            Crea con il wizard
          </Button>
        </div>
        {!wizard.available && wizard.hint && (
          <p className="text-[11px] text-muted-foreground">{wizard.hint}</p>
        )}
        {error && <p className="text-[11px] text-destructive">{error}</p>}
        <p className="text-[11px] text-muted-foreground">
          Dà all’orchestratore del workspace un’identità e un metodo su misura. Il
          wizard ti intervista e salva il risultato qui.
        </p>
      </Section>
    </SettingsPage>
  );
}
