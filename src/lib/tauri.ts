import { invoke, type Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { WorkerPermission } from "./workerPermission";
import type { BoardView, TaskAction } from "./board";
import type { Schedule, ScheduleState } from "./schedule";
import type { AutoOrchestrate } from "./workflow";

/** Avvio personalizzato di un workspace (vedi `Launch` in workspaces.rs). */
export interface Launch {
  id: string;
  name: string;
  command: string;
}

/** Avvio integrato (vedi `BuiltinLaunch` in workspaces.rs). */
export interface BuiltinLaunch {
  id: string;
  name: string;
  /** Comando eseguito ora. */
  command: string;
  /** Comando da salvare nel tab (ripristino/ripresa). */
  restore_command: string;
  agent_id: string;
}

/** Impostazioni globali degli avvii (null = shell vuota). */
export interface LaunchSettings {
  default_launch: string | null;
  /** Mostra limiti e token di utilizzo (default true). */
  show_usage: boolean;
}

export interface Workspace {
  id: string;
  name: string;
  kind: "windows" | "wsl";
  path: string;
  distro?: string | null;
  shell?: string | null;
  created_at: number;
  launches: Launch[];
  /** Avvio predefinito: assente/null = eredita il globale; "shell" = shell vuota; altrimenti id. */
  default_launch?: string | null;
  /** Tetto dei permessi dei worker automatici (vedi `Workspace` in workspaces.rs). */
  worker_permission: WorkerPermission;
  /** Modello dei worker automatici; assente/null = predefinito dell'agente. */
  worker_model?: string | null;
  /** Regole di permesso pre-approvate / vietate per i worker Claude Code. */
  worker_allow?: string[];
  worker_deny?: string[];
  /** Comando di setup dei worktree dei task (assente = nessuno). */
  worktree_setup?: string;
  /** Regole inviate a ogni worker automatico (assente = nessuna). */
  worker_rules?: string;
  /** Orchestratore automatico del workspace (assente = non automatico). */
  auto_orchestrate?: AutoOrchestrate | null;
  /** Configurazione per repository (chiave = cartella relativa alla root, "." = la root). */
  repos?: Record<string, RepoConfig>;
  /** Archiviato: nascosto dalla sidebar, board ferma (assente = attivo). */
  archived?: boolean;
}

/** Preparazione e pulizia dei worktree di un repository (vedi `RepoConfig` in workspaces.rs). */
export interface RepoConfig {
  base_branch: string | null;
  container_root: string | null;
  copy_ignored: string[];
  setup: string[];
  cleanup: string[];
  remove_force: boolean;
}

/** Messaggi sul canale dedicato di una sessione pty (vedi PtyEvent in pty.rs). */
export type PtyEvent = { kind: "output"; data: string } | { kind: "exit" };

/** Evento di un coding agent (vedi `NotifyPayload` in agent_notify.rs). */
export interface AgentEvent {
  event: "test" | "session-start" | "finished" | "needs-input" | string;
  agent: string;
  /** Id del tab (`CLI_COMPANION_SESSION`); assente per `test`. */
  tab_id?: string | null;
  agent_session_id?: string | null;
  /** Percorso assoluto del transcript della sessione, come lo vede l'agente. */
  transcript_path?: string | null;
  notification_type?: string | null;
  message?: string | null;
}

/** Finestra di un limite di utilizzo. */
export interface LimitWindow {
  /** 0-100. */
  used_percentage: number;
  /** RFC3339. */
  resets_at: string | null;
}

export interface UsageLimitsReply {
  state: "ok" | "expired" | "unavailable" | "disabled" | "unsupported";
  five_hour: LimitWindow | null;
  seven_day: LimitWindow | null;
  message: string | null;
}

export interface TokenTotals {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
}

export interface SessionUsage {
  main: TokenTotals;
  subagents: TokenTotals;
  context_tokens: number;
  model: string | null;
  messages: number;
}

/** Stato di un agente in un ambiente (vedi `AgentEnvStatus` in agents/mod.rs). */
/** Plugin installato con versione diversa da quella attesa dall'app. */
export interface OutdatedPlugin {
  agent_id: string;
  agent_name: string;
  env_key: string;
  env_label: string;
  installed_version: string | null;
  expected_version: string;
}

export interface PluginUpdateResult {
  agent_id: string;
  agent_name: string;
  env_key: string;
  env_label: string;
  ok: boolean;
  /** Testo d'errore quando !ok. */
  log: string;
}

export interface AgentEnvStatus {
  agent_id: string;
  agent_name: string;
  env_key: string;
  env_label: string;
  env_kind: "windows" | "wsl";
  found: boolean;
  version?: string | null;
  installed: boolean;
}

/** Regola del profilo di input (vedi `InputRule` in agents/mod.rs). */
export interface InputRule {
  event: string;
  env_kind: "windows" | "wsl" | null;
  send: string;
}

/** Profilo di input di un agente (vedi `AgentProfile` in agents/mod.rs). */
export interface AgentProfile {
  agent_id: string;
  agent_name: string;
  input_profile: { rules: InputRule[] };
}

/** Evento `automation-spawn` (vedi automation.rs): apri un tab worker. */
export interface AutomationSpawn {
  tab_id: string;
  workspace_id: string;
  /** Comando di avvio da digitare nella shell del tab. */
  command: string;
  cwd?: string | null;
  title: string;
  parent_tab_id: string;
  agent_id: string;
  /** Assente = worker. */
  role?: "worker" | "orchestrator";
}

/** Evento `automation-close`: chiudi il tab worker. */
export interface AutomationClose {
  tab_id: string;
}

/** Evento `automation-update` e risultato di `automation_sessions`. */
export interface AutomationTabInfo {
  tab_id: string;
  status: string;
  title: string;
  parent_tab_id: string;
  last_report?: string | null;
}

export interface NewWorkspace {
  name: string;
  kind: "windows" | "wsl";
  path: string;
  distro?: string;
  shell?: string;
}

export const api = {
  listWorkspaces: () => invoke<Workspace[]>("list_workspaces"),

  addWorkspace: (w: NewWorkspace) =>
    invoke<Workspace>("add_workspace", {
      name: w.name,
      kind: w.kind,
      path: w.path,
      distro: w.distro ?? null,
      shell: w.shell && w.shell.trim() !== "" ? w.shell : null,
    }),

  renameWorkspace: (id: string, name: string) =>
    invoke<Workspace>("rename_workspace", { id, name }),

  setWorkerModel: (id: string, model: string | null) =>
    invoke<Workspace>("set_worker_model", { id, model }),
  setWorkerPermissionRules: (id: string, allow: string[], deny: string[]) =>
    invoke<Workspace>("set_worker_permission_rules", { id, allow, deny }),
  setWorkerPermission: (id: string, mode: WorkerPermission) =>
    invoke<Workspace>("set_worker_permission", { id, mode }),

  setWorkerRules: (workspaceId: string, rules: string) =>
    invoke<Workspace>("set_worker_rules", { workspaceId, rules }),

  setWorktreeSetup: (id: string, command: string | null) =>
    invoke<Workspace>("set_worktree_setup", { id, command }),

  /** Imposta (o con null rimuove) la configurazione di un repository del workspace. */
  setRepoConfig: (workspaceId: string, repo: string, config: RepoConfig | null) =>
    invoke<Workspace>("set_repo_config", { workspaceId, repo, config }),

  setAutoOrchestrate: (id: string, config: AutoOrchestrate | null) =>
    invoke<Workspace>("set_auto_orchestrate", { id, config }),

  removeWorkspace: (id: string) => invoke<void>("remove_workspace", { id }),

  /** Nuovo ordine dei workspace visibili; il backend accoda gli altri. Restituisce l'elenco completo. */
  reorderWorkspaces: (ids: string[]) => invoke<Workspace[]>("reorder_workspaces", { ids }),

  setWorkspaceArchived: (workspaceId: string, archived: boolean) =>
    invoke<Workspace>("set_workspace_archived", { workspaceId, archived }),

  addLaunch: (workspaceId: string, name: string, command: string, isDefault: boolean) =>
    invoke<Workspace>("add_launch", { workspaceId, name, command, isDefault }),

  updateLaunch: (workspaceId: string, launchId: string, name: string, command: string) =>
    invoke<Workspace>("update_launch", { workspaceId, launchId, name, command }),

  setDefaultLaunch: (workspaceId: string, launchId: string | null) =>
    invoke<Workspace>("set_default_launch", { workspaceId, launchId }),

  builtinLaunches: () => invoke<BuiltinLaunch[]>("builtin_launches"),

  getLaunchSettings: () => invoke<LaunchSettings>("get_launch_settings"),

  setGlobalDefaultLaunch: (launchId: string | null) =>
    invoke<LaunchSettings>("set_global_default_launch", { launchId }),

  removeLaunch: (workspaceId: string, launchId: string) =>
    invoke<Workspace>("remove_launch", { workspaceId, launchId }),

  openWorkspaceFolder: (id: string) => invoke<void>("open_workspace_folder", { id }),

  listWslDistros: () => invoke<string[]>("list_wsl_distros"),

  checkWslPath: (distro: string, path: string) =>
    invoke<boolean>("check_wsl_path", { distro, path }),

  // Nota: Tauri v2 converte i nomi degli argomenti in camelCase
  // (workspace_id -> workspaceId): le chiavi qui devono esserlo già.
  ptySpawn: (
    workspaceId: string,
    tabId: string,
    cols: number,
    rows: number,
    onEvent: Channel<PtyEvent>,
    cwd?: string | null,
  ) =>
    invoke<number>("pty_spawn", { workspaceId, tabId, cols, rows, onEvent, cwd: cwd ?? null }),

  ptyWrite: (sessionId: number, data: string) =>
    invoke<void>("pty_write", { sessionId, data }),

  ptyResize: (sessionId: number, cols: number, rows: number) =>
    invoke<void>("pty_resize", { sessionId, cols, rows }),

  ptyClose: (sessionId: number) => invoke<void>("pty_close", { sessionId }),

  testAgentNotify: () => invoke<string>("test_agent_notify"),

  /** Toast cliccabile (solo Windows): al click focus + `notification-activated`. */
  showAgentNotification: (tabId: string, title: string, body: string) =>
    invoke<void>("show_agent_notification", { tabId, title, body }),

  setShowUsage: (enabled: boolean) => invoke<LaunchSettings>("set_show_usage", { enabled }),

  agentUsageLimits: (agentId: string, env: string) =>
    invoke<UsageLimitsReply>("agent_usage_limits", { agentId, env }),

  agentSessionUsage: (agentId: string, env: string, transcriptPath: string) =>
    invoke<SessionUsage | null>("agent_session_usage", { agentId, env, transcriptPath }),

  agentProfiles: () => invoke<AgentProfile[]>("agent_profiles"),

  agentEnvStatus: () => invoke<AgentEnvStatus[]>("agent_env_status"),

  installAgentPlugin: (agentId: string, envKey: string) =>
    invoke<string>("install_agent_plugin", { agentId, envKey }),

  agentPluginsOutdated: () => invoke<OutdatedPlugin[]>("agent_plugins_outdated"),

  agentPluginsUpdate: () => invoke<PluginUpdateResult[]>("agent_plugins_update"),

  removeAgentPlugin: (agentId: string, envKey: string) =>
    invoke<string>("remove_agent_plugin", { agentId, envKey }),

  // Sessioni automatiche (backend: src-tauri/src/automation.rs).
  /** Un tab automatico e' stato chiuso (utente, app o shell uscita). */
  automationTabClosed: (tabId: string) => invoke<void>("automation_tab_closed", { tabId }),

  /**
   * Registra un tab dell'avvio "Orchestratore" prima di aprirlo: fallisce se il
   * workspace ha gia' un orchestratore (automatico o manuale).
   */
  automationOrchestratorOpen: (workspaceId: string, tabId: string) =>
    invoke<void>("automation_orchestrator_open", { workspaceId, tabId }),

  automationSessions: () => invoke<AutomationTabInfo[]>("automation_sessions"),

  // Board dei task (backend: src-tauri, contratto in docs/BOARD.MD).
  boardGet: (workspaceId: string) => invoke<BoardView>("board_get", { workspaceId }),

  /** Azzera il blocco e fa aprire subito l'orchestratore se serve. */
  startBoardOrchestrator: (workspaceId: string) =>
    invoke<void>("start_board_orchestrator", { workspaceId }),

  /** Repository del workspace: "." se la root è un repo, più le sottocartelle con un .git. */
  workspaceDetectRepos: (workspaceId: string) =>
    invoke<string[]>("workspace_detect_repos", { workspaceId }),

  boardSet: (workspaceId: string, settings: { maxParallel?: number; paused?: boolean }) =>
    invoke<BoardView>("board_set", {
      workspaceId,
      maxParallel: settings.maxParallel ?? null,
      paused: settings.paused ?? null,
    }),

  boardTaskSetModel: (workspaceId: string, taskId: string, model: string | null) =>
    invoke<BoardView>("board_task_set_model", { workspaceId, taskId, model }),
  boardTaskAction: (workspaceId: string, taskId: string, action: TaskAction) =>
    invoke<BoardView>("board_task_action", { workspaceId, taskId, action }),

  // Esecuzione automatica e workflow (contratto in docs/SCHEDULE.MD).
  scheduleGet: () => invoke<ScheduleState>("schedule_get"),

  scheduleSet: (schedule: Schedule) => invoke<ScheduleState>("schedule_set", { schedule }),

  workflowGet: (workspaceId: string) => invoke<string | null>("workflow_get", { workspaceId }),

  /** `content` null o vuoto = elimina il workflow. */
  workflowSave: (workspaceId: string, content: string | null) =>
    invoke<void>("workflow_save", { workspaceId, content }),

  // Persistenza della sessione (backend: src-tauri/src/session.rs).
  loadSession: () => invoke<string | null>("load_session"),

  saveSession: (data: string) => invoke<void>("save_session", { data }),

  saveScrollback: (tabId: string, data: string) =>
    invoke<void>("save_scrollback", { tabId, data }),

  loadScrollback: (tabId: string) => invoke<string | null>("load_scrollback", { tabId }),

  deleteScrollback: (tabId: string) => invoke<void>("delete_scrollback", { tabId }),

  pruneScrollback: (keep: string[]) => invoke<void>("prune_scrollback", { keep }),
};

export function onNotificationActivated(cb: (tabId: string) => void) {
  return listen<{ tab_id: string }>("notification-activated", (event) =>
    cb(event.payload.tab_id),
  );
}

export function onAutomationSpawn(cb: (e: AutomationSpawn) => void) {
  return listen<AutomationSpawn>("automation-spawn", (event) => cb(event.payload));
}

export function onAutomationClose(cb: (e: AutomationClose) => void) {
  return listen<AutomationClose>("automation-close", (event) => cb(event.payload));
}

export function onAutomationUpdate(cb: (e: AutomationTabInfo) => void) {
  return listen<AutomationTabInfo>("automation-update", (event) => cb(event.payload));
}

export function onScheduleState(cb: (s: ScheduleState) => void) {
  return listen<ScheduleState>("schedule-state", (event) => cb(event.payload));
}

export function onBoardUpdate(cb: (b: BoardView) => void) {
  return listen<BoardView>("board-update", (event) => cb(event.payload));
}

export function onAgentEvent(cb: (e: AgentEvent) => void) {
  return listen<AgentEvent>("agent-event", (event) => cb(event.payload));
}
