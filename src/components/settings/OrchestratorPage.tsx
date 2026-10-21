import { useEffect, useState } from "react";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { api } from "../../lib/tauri";
import type { Workspace } from "../../lib/tauri";
import {
  MODEL_MAX,
  buildAutoOrchestrate,
  effectiveAutoOrchestrate,
  modelError,
  sameAutoOrchestrate,
} from "../../lib/workflow";
import { Section, SettingsPage, selectClass } from "./shared";

interface Props {
  workspace: Workspace;
  /** Agenti capaci di fare da orchestratore (id e nome). */
  agents: { id: string; name: string }[];
  onWorkspaceUpdated: (ws: Workspace) => void;
}

/** Workspace › Orchestratore: configurazione dell'orchestratore automatico. */
export default function OrchestratorPage({ workspace, agents, onWorkspaceUpdated }: Props) {
  const saved = effectiveAutoOrchestrate(workspace, agents[0]?.id);
  const [draft, setDraft] = useState({
    enabled: saved.enabled,
    agent: saved.agent,
    model: saved.model ?? "",
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // La bozza segue il workspace selezionato e il valore salvato.
  useEffect(() => {
    setDraft({ enabled: saved.enabled, agent: saved.agent, model: saved.model ?? "" });
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspace.id, saved.enabled, saved.agent, saved.model]);

  const config = buildAutoOrchestrate(draft);
  const modelProblem = modelError(draft.model);
  const dirty = !sameAutoOrchestrate(config, saved);

  const save = async () => {
    setError(null);
    setSaving(true);
    try {
      onWorkspaceUpdated(await api.setAutoOrchestrate(workspace.id, config));
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const agentOptions = agents.some((a) => a.id === draft.agent)
    ? agents
    : [...agents, { id: draft.agent, name: draft.agent }];

  return (
    <SettingsPage
      title="Orchestratore"
      description="Un orchestratore che lavora da solo in questo workspace durante le fasce orarie."
    >
      <Section>
        <label className="flex items-center gap-2 text-[13px]">
          <input
            type="checkbox"
            checked={draft.enabled}
            onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
            className="h-3.5 w-3.5 accent-primary"
          />
          Lavora da solo nelle fasce orarie
        </label>
        <div className="grid grid-cols-2 gap-3">
          <div>
            <p className="mb-1 text-xs font-semibold">Agente</p>
            <select
              value={draft.agent}
              disabled={saving}
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
            <p className="mb-1 text-xs font-semibold">Modello (facoltativo)</p>
            <Input
              value={draft.model}
              onChange={(e) => setDraft({ ...draft, model: e.target.value })}
              placeholder="predefinito dell’agente"
              maxLength={MODEL_MAX}
              className="h-9 font-mono text-[12px]"
            />
          </div>
        </div>
        <div>
          <Button
            size="sm"
            variant="secondary"
            disabled={saving || !dirty || modelProblem !== null}
            onClick={() => void save()}
          >
            Salva
          </Button>
        </div>
        {(modelProblem ?? error) && (
          <p className="text-[11px] text-destructive">{modelProblem ?? error}</p>
        )}
        <p className="text-[11px] text-muted-foreground">
          All’apertura di una fascia l’app avvia un orchestratore in questo
          workspace se c’è lavoro da portare avanti. Ha i permessi dei worker del
          workspace: per lavorare di notte serve un livello che non apra dialoghi.
          Le fasce si impostano in Impostazioni › Fasce orarie.
        </p>
      </Section>
    </SettingsPage>
  );
}
