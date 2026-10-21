import { useEffect, useState } from "react";
import { Button } from "./ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "./ui/dialog";
import AgentSettingsTab from "./AgentSettingsTab";
import UpdateCheck from "./UpdateCheck";
import ScheduleSettings from "./ScheduleSettings";
import GeneralLaunchesPage from "./settings/GeneralLaunchesPage";
import WorkspaceLaunchesPage from "./settings/WorkspaceLaunchesPage";
import WorkerPage from "./settings/WorkerPage";
import OrchestratorPage from "./settings/OrchestratorPage";
import WorkflowPage from "./settings/WorkflowPage";
import { SettingsPage, selectClass } from "./settings/shared";
import type { AgentEnvStatus, AgentProfile, BuiltinLaunch, Workspace } from "../lib/tauri";
import { cn } from "../lib/utils";

export type SettingsTab =
  | "general-launches"
  | "schedule"
  | "agents"
  | "updates"
  | "launches"
  | "worker"
  | "orchestrator"
  | "workflow";

/** Pagine della sezione Generale. */
const GENERAL_TABS: { id: SettingsTab; label: string }[] = [
  { id: "general-launches", label: "Avvii" },
  { id: "schedule", label: "Fasce orarie" },
  { id: "agents", label: "Coding agent" },
  { id: "updates", label: "Aggiornamenti" },
];

/** Pagine della sezione Workspace (riferite al workspace scelto). */
const WORKSPACE_TABS: { id: SettingsTab; label: string }[] = [
  { id: "launches", label: "Avvii" },
  { id: "worker", label: "Worker" },
  { id: "orchestrator", label: "Orchestratore" },
  { id: "workflow", label: "Workflow" },
];

const GROUP_LABEL =
  "px-2 pb-1 text-[10px] font-semibold uppercase tracking-[0.08em] text-faint";

interface Props {
  isOpen: boolean;
  initialTab?: SettingsTab;
  /** Workspace preselezionato nelle pagine del workspace. */
  initialWorkspaceId?: string;
  workspaces: Workspace[];
  onClose: () => void;
  onWorkspaceUpdated: (ws: Workspace) => void;
  /** Esito del controllo aggiornamenti fatto nella pagina "Aggiornamenti". */
  onUpdateAvailable?: (available: boolean) => void;
  /** C'è un aggiornamento disponibile (pallino sulla voce del menu). */
  updateAvailable?: boolean;
  /** Elenco aggiornato degli ambienti dei coding agent (per la barra di stato). */
  onAgentEnvs?: (envs: AgentEnvStatus[]) => void;
  /** Interruttore "Mostra utilizzo" cambiato nelle Impostazioni. */
  onShowUsage?: (enabled: boolean) => void;
  /** Agenti noti (per la scelta dell'orchestratore automatico). */
  agentProfiles?: AgentProfile[];
  /** Ambienti dei coding agent già noti (per il wizard del workflow). */
  agentEnvs?: AgentEnvStatus[];
  /** Avvii integrati dell'app. */
  builtinLaunches?: BuiltinLaunch[];
  /** Avvio predefinito globale (id di un integrato; null = shell vuota). */
  globalLaunch?: string | null;
  onGlobalLaunchChange?: (id: string | null) => void;
  /** Apre il terminale del wizard del workflow nel workspace. */
  onCreateWorkflow?: (ws: Workspace) => void;
}

export default function SettingsDialog({
  isOpen,
  initialTab = "general-launches",
  initialWorkspaceId,
  workspaces,
  onClose,
  onWorkspaceUpdated,
  onUpdateAvailable,
  updateAvailable = false,
  onAgentEnvs,
  onShowUsage,
  agentProfiles = [],
  agentEnvs = [],
  builtinLaunches = [],
  globalLaunch = null,
  onGlobalLaunchChange,
  onCreateWorkflow,
}: Props) {
  const [tab, setTab] = useState<SettingsTab>(initialTab);
  const [workspaceId, setWorkspaceId] = useState<string>("");

  // All'apertura: pagina e workspace richiesti.
  useEffect(() => {
    if (!isOpen) return;
    setTab(initialTab);
    setWorkspaceId(initialWorkspaceId ?? workspaces[0]?.id ?? "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, initialTab, initialWorkspaceId]);

  // Se il workspace selezionato sparisce, ripiega sul primo disponibile.
  const selected =
    workspaces.find((w) => w.id === workspaceId) ?? workspaces[0] ?? undefined;

  const navButton = (t: { id: SettingsTab; label: string }, disabled = false) => (
    <button
      key={t.id}
      type="button"
      disabled={disabled}
      onClick={() => setTab(t.id)}
      aria-current={tab === t.id ? "page" : undefined}
      className={cn(
        "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] transition-colors disabled:cursor-not-allowed disabled:opacity-40",
        tab === t.id
          ? "bg-accent font-medium text-foreground"
          : "text-muted-foreground hover:bg-accent/50 hover:text-foreground",
      )}
    >
      {t.label}
      {t.id === "updates" && updateAvailable && (
        <span
          className="h-1.5 w-1.5 rounded-full bg-primary"
          title="Aggiornamento disponibile"
          role="img"
          aria-label="Aggiornamento disponibile"
        />
      )}
    </button>
  );

  const noWorkspace = (
    <p className="text-[13px] text-muted-foreground">
      Nessun workspace. Aggiungi una cartella per configurarlo.
    </p>
  );

  const renderPage = () => {
    switch (tab) {
      case "general-launches":
        return (
          <GeneralLaunchesPage
            builtins={builtinLaunches}
            globalLaunch={globalLaunch}
            onGlobalLaunchChange={(id) => onGlobalLaunchChange?.(id)}
          />
        );
      case "schedule":
        return (
          <SettingsPage
            title="Fasce orarie"
            description="Quando gli orchestratori automatici dei workspace possono lavorare."
          >
            <ScheduleSettings />
          </SettingsPage>
        );
      case "agents":
        return (
          <SettingsPage
            title="Coding agent"
            description="Il plugin che avvisa l’app quando un agente ha finito o ha bisogno di te."
          >
            <AgentSettingsTab onEnvs={onAgentEnvs} onShowUsage={onShowUsage} />
          </SettingsPage>
        );
      case "updates":
        return (
          <SettingsPage
            title="Aggiornamenti"
            description="Controlla e installa le nuove versioni dell’app."
          >
            <UpdateCheck onAvailable={onUpdateAvailable} />
          </SettingsPage>
        );
    }
    if (!selected) return noWorkspace;
    switch (tab) {
      case "launches":
        return (
          <WorkspaceLaunchesPage
            workspace={selected}
            builtins={builtinLaunches}
            globalLaunch={globalLaunch}
            onWorkspaceUpdated={onWorkspaceUpdated}
          />
        );
      case "worker":
        return <WorkerPage workspace={selected} onWorkspaceUpdated={onWorkspaceUpdated} />;
      case "orchestrator":
        return (
          <OrchestratorPage
            workspace={selected}
            // Le sessioni automatiche sono supportate solo da Claude Code: i
            // profili non espongono un flag di capacita', quindi si filtra per id.
            agents={(agentProfiles.length > 0
              ? agentProfiles.map((p) => ({ id: p.agent_id, name: p.agent_name }))
              : [{ id: "claude-code", name: "Claude Code" }]
            ).filter((a) => a.id === "claude-code")}
            onWorkspaceUpdated={onWorkspaceUpdated}
          />
        );
      case "workflow":
        return (
          <WorkflowPage
            workspace={selected}
            agentEnvs={agentEnvs}
            onCreateWorkflow={(ws) => onCreateWorkflow?.(ws)}
          />
        );
    }
  };

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
    >
      <DialogContent
        className="flex h-[min(640px,85vh)] flex-col gap-0 overflow-hidden p-0 sm:max-w-[920px]"
        onCloseAutoFocus={(e) => {
          // Nessun trigger a cui tornare: il focus va al terminale attivo.
          e.preventDefault();
          document
            .querySelector<HTMLElement>('[data-focused="true"] .xterm-helper-textarea')
            ?.focus();
          // Rete di sicurezza: se nessun layer Radix e' aperto ma il body e'
          // rimasto con pointer-events bloccato, lo ripristina.
          if (
            document.body.style.pointerEvents === "none" &&
            !document.querySelector("[data-radix-popper-content-wrapper], [role='dialog'], [role='menu']")
          ) {
            document.body.style.pointerEvents = "";
          }
        }}
      >
        <DialogHeader className="border-b px-5 py-3.5">
          <DialogTitle>Impostazioni</DialogTitle>
        </DialogHeader>

        <div className="flex min-h-0 flex-1">
          <nav className="flex w-[200px] flex-shrink-0 flex-col gap-4 overflow-y-auto border-r p-3">
            <div className="flex flex-col gap-0.5">
              <p className={GROUP_LABEL}>Generale</p>
              {GENERAL_TABS.map((t) => navButton(t))}
            </div>
            <div className="flex flex-col gap-0.5">
              <p className={GROUP_LABEL}>Workspace</p>
              <select
                value={selected?.id ?? ""}
                disabled={workspaces.length === 0}
                onChange={(e) => setWorkspaceId(e.target.value)}
                aria-label="Workspace"
                className={cn(selectClass, "mb-1 h-8 text-[13px]")}
              >
                {workspaces.length === 0 && <option value="">Nessun workspace</option>}
                {workspaces.map((w) => (
                  <option key={w.id} value={w.id}>
                    {w.name}
                  </option>
                ))}
              </select>
              {WORKSPACE_TABS.map((t) => navButton(t, !selected))}
              {!selected && (
                <p className="px-2 pt-1 text-[11px] text-muted-foreground">
                  Aggiungi un workspace per configurarlo.
                </p>
              )}
            </div>
          </nav>

          <div className="min-w-0 flex-1 overflow-y-auto p-5">{renderPage()}</div>
        </div>

        <DialogFooter className="border-t px-5 py-3">
          <Button variant="outline" onClick={onClose}>
            Chiudi
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
