import { useEffect, useState } from "react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { api } from "../lib/tauri";
import type { AgentEnvStatus, Workspace } from "../lib/tauri";
import {
  MODEL_MAX,
  WORKFLOW_MAX,
  buildAutoOrchestrate,
  canSaveWorkflow,
  effectiveAutoOrchestrate,
  modelError,
  sameAutoOrchestrate,
  wizardAvailability,
  workflowStatusText,
} from "../lib/workflow";

const selectClass =
  "flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 [&>option]:bg-popover [&>option]:text-popover-foreground";

interface Props {
  workspace: Workspace;
  /** Agenti capaci di fare da orchestratore (id e nome). */
  agents: { id: string; name: string }[];
  agentEnvs: AgentEnvStatus[];
  onWorkspaceUpdated: (ws: Workspace) => void;
  /** Chiude le impostazioni e apre il terminale del wizard nel workspace. */
  onCreateWorkflow: (ws: Workspace) => void;
}

/** Orchestratore automatico e workflow di un workspace. */
export default function WorkspaceAutomationSettings({
  workspace,
  agents,
  agentEnvs,
  onWorkspaceUpdated,
  onCreateWorkflow,
}: Props) {
  const saved = effectiveAutoOrchestrate(workspace, agents[0]?.id);
  const [draft, setDraft] = useState({
    enabled: saved.enabled,
    agent: saved.agent,
    model: saved.model ?? "",
  });
  const [orchSaving, setOrchSaving] = useState(false);
  const [orchError, setOrchError] = useState<string | null>(null);

  // La bozza segue il workspace selezionato e il valore salvato.
  useEffect(() => {
    setDraft({ enabled: saved.enabled, agent: saved.agent, model: saved.model ?? "" });
    setOrchError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspace.id, saved.enabled, saved.agent, saved.model]);

  const config = buildAutoOrchestrate(draft);
  const modelProblem = modelError(draft.model);
  const orchDirty = !sameAutoOrchestrate(config, saved);

  const saveOrchestrator = async () => {
    setOrchError(null);
    setOrchSaving(true);
    try {
      onWorkspaceUpdated(await api.setAutoOrchestrate(workspace.id, config));
    } catch (e) {
      setOrchError(String(e));
    } finally {
      setOrchSaving(false);
    }
  };

  // Workflow: testo salvato (null = assente) e bozza dell'editor.
  const [savedWorkflow, setSavedWorkflow] = useState<string | null>(null);
  const [workflowDraft, setWorkflowDraft] = useState("");
  const [workflowLoading, setWorkflowLoading] = useState(true);
  const [workflowBusy, setWorkflowBusy] = useState(false);
  const [workflowError, setWorkflowError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setWorkflowLoading(true);
    setWorkflowError(null);
    setConfirmDelete(false);
    api
      .workflowGet(workspace.id)
      .then((w) => {
        if (cancelled) return;
        setSavedWorkflow(w);
        setWorkflowDraft(w ?? "");
      })
      .catch((e) => {
        if (cancelled) return;
        setSavedWorkflow(null);
        setWorkflowDraft("");
        setWorkflowError(String(e));
      })
      .finally(() => !cancelled && setWorkflowLoading(false));
    return () => {
      cancelled = true;
    };
  }, [workspace.id]);

  const saveWorkflow = async () => {
    setWorkflowError(null);
    setWorkflowBusy(true);
    try {
      await api.workflowSave(workspace.id, workflowDraft);
      setSavedWorkflow(workflowDraft);
    } catch (e) {
      setWorkflowError(String(e));
    } finally {
      setWorkflowBusy(false);
    }
  };

  const deleteWorkflow = async () => {
    setWorkflowError(null);
    setWorkflowBusy(true);
    try {
      await api.workflowSave(workspace.id, null);
      setSavedWorkflow(null);
      setWorkflowDraft("");
      setConfirmDelete(false);
    } catch (e) {
      setWorkflowError(String(e));
    } finally {
      setWorkflowBusy(false);
    }
  };

  const wizard = wizardAvailability(workspace, agentEnvs);
  const agentOptions = agents.some((a) => a.id === draft.agent)
    ? agents
    : [...agents, { id: draft.agent, name: draft.agent }];

  return (
    <>
      <div>
        <p className="mb-1 text-xs font-semibold">Orchestratore automatico</p>
        <label className="flex items-center gap-2 text-[13px]">
          <input
            type="checkbox"
            checked={draft.enabled}
            onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
            className="h-3.5 w-3.5 accent-primary"
          />
          Lavora da solo nelle fasce orarie
        </label>
        <div className="mt-2 grid grid-cols-2 gap-2">
          <div>
            <p className="mb-1 text-[11px] text-muted-foreground">Agente</p>
            <select
              value={draft.agent}
              disabled={orchSaving}
              onChange={(e) => setDraft({ ...draft, agent: e.target.value })}
              className={selectClass}
            >
              {agentOptions.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          </div>
          <div>
            <p className="mb-1 text-[11px] text-muted-foreground">Modello (facoltativo)</p>
            <Input
              value={draft.model}
              onChange={(e) => setDraft({ ...draft, model: e.target.value })}
              placeholder="predefinito dell’agente"
              maxLength={MODEL_MAX}
              className="h-9 font-mono text-[12px]"
            />
          </div>
        </div>
        <div className="mt-2">
          <Button
            size="sm"
            variant="secondary"
            disabled={orchSaving || !orchDirty || modelProblem !== null}
            onClick={() => void saveOrchestrator()}
          >
            Salva
          </Button>
        </div>
        {(modelProblem ?? orchError) && (
          <p className="mt-1 text-[11px] text-destructive">{modelProblem ?? orchError}</p>
        )}
        <p className="mt-1 text-[11px] text-muted-foreground">
          All’apertura di una fascia l’app avvia un orchestratore in questo
          workspace se c’è lavoro da portare avanti. Ha i permessi dei worker del
          workspace: per lavorare di notte serve un livello che non apra dialoghi.
          Le fasce si impostano in Impostazioni › Esecuzione automatica.
        </p>
      </div>

      <div>
        <div className="mb-1 flex items-center justify-between">
          <p className="text-xs font-semibold">Workflow</p>
          <span className="text-[11px] text-muted-foreground">
            {workflowLoading ? "Caricamento…" : workflowStatusText(savedWorkflow)}
          </span>
        </div>
        <textarea
          value={workflowDraft}
          onChange={(e) => setWorkflowDraft(e.target.value)}
          disabled={workflowLoading}
          maxLength={WORKFLOW_MAX}
          rows={8}
          spellCheck={false}
          placeholder="Metodo di lavoro dell’orchestratore di questo workspace (Markdown, in inglese)"
          className="w-full resize-y rounded-md border border-input bg-transparent px-3 py-2 font-mono text-[12px] leading-relaxed shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
        />
        <div className="mt-1 flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            disabled={workflowBusy || workflowLoading || !canSaveWorkflow(workflowDraft, savedWorkflow)}
            onClick={() => void saveWorkflow()}
          >
            Salva
          </Button>
          {confirmDelete ? (
            <>
              <Button
                size="sm"
                variant="destructive"
                disabled={workflowBusy}
                onClick={() => void deleteWorkflow()}
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
              disabled={workflowBusy || workflowLoading || savedWorkflow === null}
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
          <p className="mt-1 text-[11px] text-muted-foreground">{wizard.hint}</p>
        )}
        {workflowError && <p className="mt-1 text-[11px] text-destructive">{workflowError}</p>}
        <p className="mt-1 text-[11px] text-muted-foreground">
          Dà all’orchestratore del workspace un’identità e un metodo su misura. Il
          wizard ti intervista e salva il risultato qui.
        </p>
      </div>
    </>
  );
}
