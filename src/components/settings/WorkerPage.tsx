import { useEffect, useMemo, useState } from "react";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { api } from "../../lib/tauri";
import type { Workspace } from "../../lib/tauri";
import {
  WORKER_PERMISSIONS,
  normalizeWorkerPermission,
  parseWorkerRules,
  rulesToText,
  workerPermissionLabel,
} from "../../lib/workerPermission";
import type { WorkerPermission } from "../../lib/workerPermission";
import { modelError, sanitizeModel } from "../../lib/workflow";
import {
  WORKER_RULES_MAX,
  WORKTREE_SETUP_MAX,
  sanitizeWorkerRules,
  sanitizeWorktreeSetup,
} from "../../lib/board";
import { Field, Section, SettingsPage, selectClass, useWorkspaceOp } from "./shared";

const rulesAreaClass =
  "w-full resize-y rounded-md border border-input bg-transparent px-2 py-1.5 font-mono text-[12px] outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50";

const sameList = (a: readonly string[], b: readonly string[]) =>
  a.length === b.length && a.every((x, i) => x === b[i]);

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
  const [allowDraft, setAllowDraft] = useState("");
  const [denyDraft, setDenyDraft] = useState("");
  const [rulesDraft, setRulesDraft] = useState("");
  const [rulesSaved, setRulesSaved] = useState(false);

  useEffect(() => {
    setRulesDraft(workspace.worker_rules ?? "");
    setRulesSaved(false);
  }, [workspace.id, workspace.worker_rules]);

  // Salva all'uscita dal campo, solo se cambiato; non blocca la pagina.
  const saveRules = () => {
    const rules = sanitizeWorkerRules(rulesDraft);
    if (rules === (workspace.worker_rules ?? "")) return;
    void run(() => api.setWorkerRules(workspace.id, rules)).then((ok) => setRulesSaved(ok));
  };

  useEffect(() => {
    setSetupDraft(workspace.worktree_setup ?? "");
  }, [workspace.id, workspace.worktree_setup]);

  useEffect(() => {
    setWorkerModelDraft(workspace.worker_model ?? "");
  }, [workspace.id, workspace.worker_model]);

  useEffect(() => {
    setAllowDraft(rulesToText(workspace.worker_allow));
    setDenyDraft(rulesToText(workspace.worker_deny));
  }, [workspace.id, workspace.worker_allow, workspace.worker_deny]);

  const permission = normalizeWorkerPermission(workspace.worker_permission);
  const allowParsed = useMemo(() => parseWorkerRules(allowDraft), [allowDraft]);
  const denyParsed = useMemo(() => parseWorkerRules(denyDraft), [denyDraft]);
  const rulesChanged =
    !sameList(allowParsed.rules, workspace.worker_allow ?? []) ||
    !sameList(denyParsed.rules, workspace.worker_deny ?? []);

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
          label="Comandi pre-approvati"
          help="Solo worker Claude Code. Una regola per riga, nella sintassi dei permessi di Claude Code: Bash(git:*), Bash(docker exec:*), Edit, WebFetch(domain:example.com). Un prefisso come Bash(docker exec:*) consente qualsiasi comando che inizia così (anche docker exec … mysql): scegli prefissi stretti. Un * in mezzo al comando può non funzionare nelle versioni vecchie di Claude Code. I divieti vincono sempre sui consentiti, ma non sono un confine di sicurezza (lo stesso programma lanciato in altra forma, es. con un path o dentro sh -c, può sfuggire). Valgono per i worker avviati da ora in poi."
        >
          {permission === "bypass" && (
            <p className="mb-2 text-[11px] text-muted-foreground">
              Con i permessi a bypass i worker non chiedono mai conferma: i Consentiti non servono,
              i Vietati restano in vigore.
            </p>
          )}
          <div className="grid gap-3 md:grid-cols-2">
            <div>
              <div className="mb-1 text-[11px] font-medium">Consentiti</div>
              <textarea
                value={allowDraft}
                onChange={(e) => setAllowDraft(e.target.value)}
                disabled={permission === "bypass"}
                rows={5}
                spellCheck={false}
                placeholder={"Bash(git:*)\nBash(npm run:*)\nBash(docker exec freud-manager:*)"}
                className={rulesAreaClass}
              />
              {allowParsed.error && (
                <p className="mt-1 text-[11px] text-destructive">{allowParsed.error}</p>
              )}
            </div>
            <div>
              <div className="mb-1 text-[11px] font-medium">Vietati</div>
              <textarea
                value={denyDraft}
                onChange={(e) => setDenyDraft(e.target.value)}
                rows={5}
                spellCheck={false}
                placeholder={"Bash(rm -rf:*)\nBash(git push:*)\nRead(./.env)"}
                className={rulesAreaClass}
              />
              {denyParsed.error && (
                <p className="mt-1 text-[11px] text-destructive">{denyParsed.error}</p>
              )}
            </div>
          </div>
          <div className="mt-2">
            <Button
              size="sm"
              variant="secondary"
              disabled={
                saving ||
                allowParsed.error !== null ||
                denyParsed.error !== null ||
                !rulesChanged
              }
              onClick={() =>
                void run(() =>
                  api.setWorkerPermissionRules(workspace.id, allowParsed.rules, denyParsed.rules),
                )
              }
            >
              Salva
            </Button>
          </div>
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

      <Section>
        <Field
          label="Regole per i worker"
          help="Test vietati, comandi da non lanciare… inviate a ogni worker automatico (di board o avviato a mano) insieme alle sue istruzioni. Valgono per i nuovi worker; l’orchestratore non deve ripeterle nei task."
        >
          <textarea
            value={rulesDraft}
            onChange={(e) => {
              setRulesDraft(e.target.value);
              setRulesSaved(false);
            }}
            onBlur={saveRules}
            rows={5}
            maxLength={WORKER_RULES_MAX}
            placeholder="Es. Non lanciare i test e2e. Non usare npm install: usa pnpm."
            aria-label="Regole per i worker"
            className="flex w-full resize-y rounded-md border border-input bg-transparent px-3 py-2 text-[12px] shadow-sm transition-colors placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          />
          <p className="mt-1 flex justify-between text-[11px] text-muted-foreground">
            <span>{rulesSaved && !saving ? "Salvato" : saving ? "Salvataggio…" : "Si salva all’uscita dal campo"}</span>
            <span className="font-mono">
              {rulesDraft.length}/{WORKER_RULES_MAX}
            </span>
          </p>
        </Field>
      </Section>

      {error && <p className="text-xs text-destructive">{error}</p>}
    </SettingsPage>
  );
}
