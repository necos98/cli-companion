import { invoke, type Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  DownloadResult,
  InstallResult,
  UpdateConfig,
  UpdateStatus,
} from "./updater";

/** Avvio personalizzato di un workspace (vedi `Launch` in workspaces.rs). */
export interface Launch {
  id: string;
  name: string;
  command: string;
  is_default: boolean;
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
  notification_type?: string | null;
  message?: string | null;
}

/** Stato di un agente in un ambiente (vedi `AgentEnvStatus` in agents/mod.rs). */
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

  removeWorkspace: (id: string) => invoke<void>("remove_workspace", { id }),

  addLaunch: (workspaceId: string, name: string, command: string, isDefault: boolean) =>
    invoke<Workspace>("add_launch", { workspaceId, name, command, isDefault }),

  updateLaunch: (
    workspaceId: string,
    launchId: string,
    name: string,
    command: string,
    isDefault: boolean,
  ) =>
    invoke<Workspace>("update_launch", {
      workspaceId,
      launchId,
      name,
      command,
      isDefault,
    }),

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

  agentProfiles: () => invoke<AgentProfile[]>("agent_profiles"),

  agentEnvStatus: () => invoke<AgentEnvStatus[]>("agent_env_status"),

  installAgentPlugin: (agentId: string, envKey: string) =>
    invoke<string>("install_agent_plugin", { agentId, envKey }),

  removeAgentPlugin: (agentId: string, envKey: string) =>
    invoke<string>("remove_agent_plugin", { agentId, envKey }),

  // Sessioni automatiche (backend: src-tauri/src/automation.rs).
  /** Un tab automatico e' stato chiuso (utente, app o shell uscita). */
  automationTabClosed: (tabId: string) => invoke<void>("automation_tab_closed", { tabId }),

  automationSessions: () => invoke<AutomationTabInfo[]>("automation_sessions"),

  // Persistenza della sessione (backend: src-tauri/src/session.rs).
  loadSession: () => invoke<string | null>("load_session"),

  saveSession: (data: string) => invoke<void>("save_session", { data }),

  saveScrollback: (tabId: string, data: string) =>
    invoke<void>("save_scrollback", { tabId, data }),

  loadScrollback: (tabId: string) => invoke<string | null>("load_scrollback", { tabId }),

  deleteScrollback: (tabId: string) => invoke<void>("delete_scrollback", { tabId }),

  pruneScrollback: (keep: string[]) => invoke<void>("prune_scrollback", { keep }),

  // Updater GitHub Releases (backend: src-tauri/src/updater.rs).
  getUpdateConfig: () => invoke<UpdateConfig>("get_update_config"),

  checkUpdate: () => invoke<UpdateStatus>("check_update"),

  downloadUpdate: (assetName: string | null) =>
    invoke<DownloadResult>("download_update", { assetName }),

  installUpdate: (filePath: string, launch: boolean) =>
    invoke<InstallResult>("install_update", { filePath, launch }),
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

export function onAgentEvent(cb: (e: AgentEvent) => void) {
  return listen<AgentEvent>("agent-event", (event) => cb(event.payload));
}
