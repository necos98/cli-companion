import { useEffect, useState } from "react";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { api } from "../../lib/tauri";
import type { Workspace } from "../../lib/tauri";
import {
  WORKER_PERMISSIONS,
  normalizeWorkerPermission,
  workerPermissionLabel,
} from "../../lib/workerPermission";
import type { WorkerPermission } from "../../lib/workerPermission";
import { modelError, sanitizeModel } from "../../lib/workflow";
import { WORKTREE_SETUP_MAX, sanitizeWorktreeSetup } from "../../lib/board";
import { Field, Section, SettingsPage, selectClass, useWorkspaceOp } from "./shared";

interface Props {
  workspace: Workspace;
  onWorkspaceUpdated: (ws: Workspace) => void;
}

/** Workspace › Worker: permessi, modello e setup dei worktree dei worker automatici. */
export default function WorkerPage({ workspace, onWorkspaceUpdated }: Props) {
  const { run, saving, error } = useWorkspaceOp(onWorkspaceUpdated);

  // Bozze: seguono il workspace selezionato e il valore salvato.
  const [setupDraft, setSetupDraft] = useState("");
  const [workerModelDraft, setWorkerModelDraft] = useState("");

  useEffect(() => {
    setSetupDraft(workspace.worktree_setup ?? "");
  }, [workspace.id, workspace.worktree_setup]);

  useEffect(() => {
    setWorkerModelDraft(workspace.worker_model ?? "");
  }, [workspace.id, workspace.worker_model]);

  const permission = normalizeWorkerPermission(workspace.worker_permission);

  return (
    <SettingsPage
      title="Worker"
      description="Come lavorano le sessioni automatiche avviate da un orchestratore in questo workspace."
    >
      <Section>
        <Field
          label="Permessi dei worker automatici"
          help="Livello massimo delle sessioni avviate da un orchestratore in questo workspace: l’orchestratore può solo restringerlo."
        >
          <select
            value={permission}
            disabled={saving}
            onChange={(e) =>
              void run(() =>
                api.setWorkerPermission(workspace.id, e.target.value as WorkerPermission),
              )
            }
            className={selectClass}
          >
            {WORKER_PERMISSIONS.map((m) => (
              <option key={m} value={m}>
                {workerPermissionLabel(m)}
              </option>
            ))}
          </select>
          {permission === "bypass" && (
            <p className="mt-1 text-[11px] text-destructive">
              I worker eseguiranno qualsiasi comando senza conferma. Claude Code
              mostra una conferma una tantum, a meno che in
              ~/.claude/settings.json ci sia skipDangerousModePermissionPrompt: true.
            </p>
          )}
        </Field>
      </Section>

      <Section>
        <Field
          label="Modello dei worker"
          help="Modello dei worker automatici di questo workspace. Vuoto = predefinito dell’agente. L’orchestratore può comunque scegliere un modello per singolo task."
        >
          <div className="flex gap-2">
            <Input
              value={workerModelDraft}
              onChange={(e) => setWorkerModelDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") e.currentTarget.blur();
              }}
              placeholder="predefinito dell’agente (es. sonnet, opus)"
              className="h-8 font-mono text-[12px]"
            />
            <Button
              size="sm"
              variant="secondary"
              disabled={
                saving ||
                modelError(workerModelDraft) !== null ||
                sanitizeModel(workerModelDraft) === (workspace.worker_model ?? null)
              }
              onClick={() =>
                void run(() => api.setWorkerModel(workspace.id, sanitizeModel(workerModelDraft)))
              }
            >
              Salva
            </Button>
          </div>
          {modelError(workerModelDraft) && (
            <p className="mt-1 text-[11px] text-destructive">{modelError(workerModelDraft)}</p>
          )}
        </Field>
      </Section>

      <Section>
        <Field
          label="Setup dei worktree"
          help="Comando dato ai worker come primo passo nei nuovi worktree dei task della board (es. installare le dipendenze). Vuoto = nessuno."
        >
          <div className="flex gap-2">
            <Input
              value={setupDraft}
              onChange={(e) => setSetupDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") e.currentTarget.blur();
              }}
              placeholder="npm ci"
              maxLength={WORKTREE_SETUP_MAX}
              className="h-8 font-mono text-[12px]"
            />
            <Button
              size="sm"
              variant="secondary"
              disabled={saving || sanitizeWorktreeSetup(setupDraft) === (workspace.worktree_setup ?? "")}
              onClick={() =>
                void run(() =>
                  api.setWorktreeSetup(workspace.id, sanitizeWorktreeSetup(setupDraft) || null),
                )
              }
            >
              Salva
            </Button>
          </div>
        </Field>
      </Section>

      {error && <p className="text-xs text-destructive">{error}</p>}
    </SettingsPage>
  );
}
