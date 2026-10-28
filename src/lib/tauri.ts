import { invoke, type Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { WorkerPermission } from "./workerPermission";
import type {
  BoardView,
  Delivery,
  E2eRun,
  NewTaskSpec,
  TaskAction,
  TaskNotifyEvent,
  TaskPatch,
} from "./board";
import type { Schedule, ScheduleState } from "./schedule";

/** Esito di `settings_proposal_decide`: `workspace` solo se la proposta è stata applicata. */
export interface SettingsDecision {
  board: BoardView;
  workspace: Workspace | null;
}

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
  /** Regole inviate a ogni worker automatico (assente = nessuna). */
  worker_rules?: string;
  /** Consegna dei task (assente = commit sul branch corrente, un worker alla volta). */
  delivery?: Delivery;
  /** La coda rispetta le fasce orarie globali (assente = no). */
  use_schedule?: boolean;
  /** Configurazione per repository (chiave = cartella relativa alla root, "." = la root). */
  repos?: Record<string, RepoConfig>;
  /** Archiviato: nascosto dalla sidebar, coda ferma (assente = attivo). */
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
  /** Test E2E facoltativi del repository; assente = nessuno. */
  e2e?: E2eConfig;
}

/** Come l'app esegue gli E2E di un repository (vedi `E2eConfig` in workspaces/e2e.rs). */
export interface E2eConfig {
  command: string;
  /** Percorso del report JUnit, relativo alla cartella del task. */
  junit: string;
  timeout_min: number;
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

/** Evento `automation-spawn` (vedi automation/host.rs): apri un tab worker. */
export interface AutomationSpawn {
  tab_id: string;
  workspace_id: string;
  /** Comando di avvio da digitare nella shell del tab. */
  command: string;
  cwd?: string | null;
  title: string;
  /** Sempre vuoto (nessun orchestratore): tenuto per compatibilita'. */
  parent_tab_id: string;
  agent_id: string;
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

  /** Modalita' di consegna dei task del workspace. */
  setDelivery: (id: string, delivery: Delivery) =>
    invoke<Workspace>("set_delivery", { id, delivery }),

  /** La coda del workspace rispetta (o no) le fasce orarie globali. */
  setUseSchedule: (id: string, value: boolean) =>
    invoke<Workspace>("set_use_schedule", { id, value }),

  /** Imposta (o con null rimuove) la configurazione di un repository del workspace. */
  setRepoConfig: (workspaceId: string, repo: string, config: RepoConfig | null) =>
    invoke<Workspace>("set_repo_config", { workspaceId, repo, config }),

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

  automationSessions: () => invoke<AutomationTabInfo[]>("automation_sessions"),

  // Coda dei task (backend: src-tauri, contratto in docs/BOARD.MD).
  boardGet: (workspaceId: string) => invoke<BoardView>("board_get", { workspaceId }),

  /** Avvia a mano gli E2E di un repository ("." = root); ritorna l'esecuzione in corso. */
  e2eRunRepo: (workspaceId: string, repo: string) => invoke<E2eRun>("e2e_run_repo", { workspaceId, repo }),

  /** Ultimo esito manuale degli E2E di un repository, se c'è. */
  e2eLastResult: (workspaceId: string, repo: string) =>
    invoke<E2eRun | null>("e2e_last_result", { workspaceId, repo }),

  /** Apre il report JUnit dell'ultima esecuzione E2E del task, o senza `taskId` di quella manuale del repository. */
  e2eOpenReport: (workspaceId: string, taskId?: string, repo?: string) =>
    invoke<void>("e2e_open_report", { workspaceId, taskId, repo }),

  /** Repository del workspace: "." se la root è un repo, più le sottocartelle con un .git. */
  workspaceDetectRepos: (workspaceId: string) =>
    invoke<string[]>("workspace_detect_repos", { workspaceId }),

  boardSet: (workspaceId: string, settings: { maxParallel?: number; paused?: boolean }) =>
    invoke<BoardView>("board_set", {
      workspaceId,
      maxParallel: settings.maxParallel ?? null,
      paused: settings.paused ?? null,
    }),

  /** Task manuale (senza proposta); l'id lo genera il backend dal titolo. */
  taskCreate: (workspaceId: string, spec: NewTaskSpec) =>
    invoke<BoardView>("task_create", { workspaceId, spec }),

  /** Solo i task Da fare / Falliti; `repo` e `model` a null tornano al valore predefinito. */
  taskUpdate: (workspaceId: string, taskId: string, patch: TaskPatch) =>
    invoke<BoardView>("task_update", { workspaceId, taskId, patch }),

  taskAction: (workspaceId: string, taskId: string, action: TaskAction) =>
    invoke<BoardView>("task_action", { workspaceId, taskId, action }),

  /** Riordina un task Da fare: prima di `beforeId`, o in fondo se assente. */
  taskMove: (workspaceId: string, taskId: string, beforeId?: string | null) =>
    invoke<BoardView>("task_move", { workspaceId, taskId, beforeId: beforeId ?? null }),

  /** Accetta o scarta una proposta di Task Intake. */
  proposalDecide: (workspaceId: string, proposalId: string, accept: boolean) =>
    invoke<BoardView>("proposal_decide", { workspaceId, proposalId, accept }),

  /** Applica o scarta una proposta di modifica alle impostazioni. */
  settingsProposalDecide: (workspaceId: string, proposalId: string, accept: boolean) =>
    invoke<SettingsDecision>("settings_proposal_decide", { workspaceId, proposalId, accept }),

  // Fasce orarie globali (solo come gate dello scheduler, docs/BOARD.MD).
  scheduleGet: () => invoke<ScheduleState>("schedule_get"),

  scheduleSet: (schedule: Schedule) => invoke<ScheduleState>("schedule_set", { schedule }),

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

/** Un'esecuzione manuale degli E2E è partita o finita. */
export interface E2eUpdateEvent {
  workspace_id: string;
  /** Chiave del repository ("." = root). */
  repo: string;
  run: E2eRun;
}

export function onE2eUpdate(cb: (e: E2eUpdateEvent) => void) {
  return listen<E2eUpdateEvent>("e2e-update", (event) => cb(event.payload));
}

/** Un task richiede attenzione o e' fallito: notifica di sistema. */
export function onTaskNotify(cb: (e: TaskNotifyEvent) => void) {
  return listen<TaskNotifyEvent>("task-notify", (event) => cb(event.payload));
}

export function onAgentEvent(cb: (e: AgentEvent) => void) {
  return listen<AgentEvent>("agent-event", (event) => cb(event.payload));
}
