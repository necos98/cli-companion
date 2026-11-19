import { invoke, type Channel } from "@tauri-apps/api/core";
import type { NotifyPayload as AgentEvent } from "../bindings/NotifyPayload.ts";
import type { AgentEnvStatus } from "../bindings/AgentEnvStatus.ts";
import type { AgentProfile } from "../bindings/AgentProfile.ts";
import type { TabEvent as AutomationClose } from "../bindings/TabEvent.ts";
import type { SpawnEvent as AutomationSpawn } from "../bindings/SpawnEvent.ts";
import type { AutomationTabInfo } from "../bindings/AutomationTabInfo.ts";
import type { AgentBot } from "../bindings/AgentBot.ts";
import type { AgentBotDraft } from "../bindings/AgentBotDraft.ts";
import type { BotLaunch } from "../bindings/BotLaunch.ts";
import type { BotSettings } from "../bindings/BotSettings.ts";
import type { BotSkillOption } from "../bindings/BotSkillOption.ts";
import type { BotTabLaunch } from "../bindings/BotTabLaunch.ts";
import type { BotTabRestore } from "../bindings/BotTabRestore.ts";
import type { BuiltinLaunch } from "../bindings/BuiltinLaunch.ts";
import type { ConnectorActionId } from "../bindings/ConnectorActionId.ts";
import type { ConnectorActionOutcome } from "../bindings/ConnectorActionOutcome.ts";
import type { ConnectorConfigField } from "../bindings/ConnectorConfigField.ts";
import type { ConnectorDeleteOutcome } from "../bindings/ConnectorDeleteOutcome.ts";
import type { ConnectorDescriptor } from "../bindings/ConnectorDescriptor.ts";
import type { ConnectorInstanceView } from "../bindings/ConnectorInstanceView.ts";
import type { ConnectorLink } from "../bindings/ConnectorLink.ts";
import type { ConnectorRoute } from "../bindings/ConnectorRoute.ts";
import type { ConnectorRouteInput } from "../bindings/ConnectorRouteInput.ts";
import type { ConnectorStatus } from "../bindings/ConnectorStatus.ts";
import type { Delivery } from "../bindings/Delivery.ts";
import type { E2eEvent as E2eUpdateEvent } from "../bindings/E2eEvent.ts";
import type { HubActionRequest } from "../bindings/HubActionRequest.ts";
import type { HubAttachment } from "../bindings/HubAttachment.ts";
import type { HubEvent } from "../bindings/HubEvent.ts";
import type { HubLanguage } from "../bindings/HubLanguage.ts";
import type { HubMessage } from "../bindings/HubMessage.ts";
import type { HubMessageOrigin } from "../bindings/HubMessageOrigin.ts";
import type { HubNotice } from "../bindings/HubNotice.ts";
import type { HubSecretary } from "../bindings/HubSecretary.ts";
import type { HubSecretaryChannels } from "../bindings/HubSecretaryChannels.ts";
import type { HubSecretaryPreset } from "../bindings/HubSecretaryPreset.ts";
import type { HubSecretaryStatus } from "../bindings/HubSecretaryStatus.ts";
import type { HubSecretaryTool } from "../bindings/HubSecretaryTool.ts";
import type { HubSettings } from "../bindings/HubSettings.ts";
import type { HubSettingsView } from "../bindings/HubSettingsView.ts";
import type { HubThread } from "../bindings/HubThread.ts";
import type { HubThreadRef } from "../bindings/HubThreadRef.ts";
import type { HubThreadSummary } from "../bindings/HubThreadSummary.ts";
import type { IntegrationEnv } from "../bindings/IntegrationEnv.ts";
import type { IntegrationInfo } from "../bindings/IntegrationInfo.ts";
import type { IntegrationsState } from "../bindings/IntegrationsState.ts";
import type { IpcStatus } from "../bindings/IpcStatus.ts";
import type { LaunchSettings } from "../bindings/LaunchSettings.ts";
import type { OutdatedPlugin } from "../bindings/OutdatedPlugin.ts";
import type { PermissionMode } from "../bindings/PermissionMode.ts";
import type { PluginUpdateResult } from "../bindings/PluginUpdateResult.ts";
import type { PtyEvent } from "../bindings/PtyEvent.ts";
import type { RepoConfig } from "../bindings/RepoConfig.ts";
import type { Schedule } from "../bindings/Schedule.ts";
import type { ScheduleState } from "../bindings/ScheduleState.ts";
import type { SessionUsage } from "../bindings/SessionUsage.ts";
import type { SettingsDecision } from "../bindings/SettingsDecision.ts";
import type { StoreWarning } from "../bindings/StoreWarning.ts";
import type { TargetKind } from "../bindings/TargetKind.ts";
import type { UsageLimitsReply } from "../bindings/UsageLimitsReply.ts";
import type { Workspace } from "../bindings/Workspace.ts";
import type { BoardView, E2eRun, NewTaskSpec, TaskAction, TaskNotifyEvent, TaskPatch } from "./board.ts";
import type { ChosenLocation } from "./workspaceLocation.ts";

// I tipi dei dati scambiati con il backend sono generati dal Rust (src/bindings):
// qui prendono i nomi usati dall'interfaccia.
export type {
  AgentBot,
  AgentBotDraft,
  AgentEnvStatus,
  AgentEvent,
  AgentProfile,
  AutomationClose,
  AutomationSpawn,
  AutomationTabInfo,
  BotLaunch,
  BotSettings,
  BotSkillOption,
  BotTabLaunch,
  BuiltinLaunch,
  ConnectorActionId,
  ConnectorActionOutcome,
  ConnectorConfigField,
  ConnectorDeleteOutcome,
  ConnectorDescriptor,
  ConnectorInstanceView,
  ConnectorLink,
  ConnectorRoute,
  ConnectorRouteInput,
  ConnectorStatus,
  Delivery,
  HubAttachment,
  HubEvent,
  HubLanguage,
  HubMessage,
  HubMessageOrigin,
  HubNotice,
  HubSecretary,
  HubSecretaryChannels,
  HubSecretaryPreset,
  HubSecretaryStatus,
  HubSecretaryTool,
  HubSettings,
  HubSettingsView,
  HubThread,
  HubThreadRef,
  HubThreadSummary,
  IntegrationEnv,
  IntegrationInfo,
  IntegrationsState,
  OutdatedPlugin,
  PluginUpdateResult,
  PtyEvent,
  RepoConfig,
  SessionUsage,
  UsageLimitsReply,
  Workspace,
};
export type { DeliveryMode } from "../bindings/DeliveryMode.ts";
export type { E2eConfig } from "../bindings/E2eConfig.ts";
export type { Launch } from "../bindings/Launch.ts";
export type { MergeMethod } from "../bindings/MergeMethod.ts";

/** Argomenti di `add_workspace` (nessun DTO Rust: il comando prende i campi sciolti). */
export interface NewWorkspace {
  name: string;
  kind: TargetKind;
  path: string;
  distro?: string;
  shell?: string;
}

/** Header con il nome del file di `hub_stage_attachment_bytes` (`FILE_NAME_HEADER` in `hub/attachments/commands.rs`). */
const HUB_FILE_NAME_HEADER = "x-hub-file-name";

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

  renameWorkspace: (id: string, name: string) => invoke<Workspace>("rename_workspace", { id, name }),

  /** Collega il workspace a un'altra cartella (anche cambiando ambiente); il resto del workspace non cambia. */
  setWorkspaceLocation: (id: string, location: ChosenLocation) =>
    invoke<Workspace>("set_workspace_location", {
      id,
      kind: location.kind,
      path: location.path,
      distro: location.distro ?? null,
    }),

  /** Agente dei worker del workspace (`null` = il predefinito globale). */
  setWorkspaceAgent: (id: string, agent: string | null) => invoke<Workspace>("set_workspace_agent", { id, agent }),
  setWorkerModel: (id: string, agent: string, model: string | null) =>
    invoke<Workspace>("set_worker_model", { id, agent, model }),
  setWorkerPermissionRules: (id: string, agent: string, allow: string[], deny: string[]) =>
    invoke<Workspace>("set_worker_permission_rules", { id, agent, allow, deny }),
  setWorkerPermission: (id: string, mode: PermissionMode) => invoke<Workspace>("set_worker_permission", { id, mode }),

  setWorkerRules: (workspaceId: string, rules: string) => invoke<Workspace>("set_worker_rules", { workspaceId, rules }),

  /** Modalita' di consegna dei task del workspace. */
  setDelivery: (id: string, delivery: Delivery) => invoke<Workspace>("set_delivery", { id, delivery }),

  /** La coda del workspace rispetta (o no) le fasce orarie globali. */
  setUseSchedule: (id: string, value: boolean) => invoke<Workspace>("set_use_schedule", { id, value }),

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

  /** Avvii integrati; `workspaceAgent` è l'agente proprio del workspace (`null` = nessuno). */
  builtinLaunches: () => invoke<BuiltinLaunch[]>("builtin_launches"),
  /** Voci di avvio dei Bot agente per un workspace con quell'agente (`null` = il predefinito). */
  botLaunches: (workspaceAgent: string | null) => invoke<BotLaunch[]>("bot_launches", { workspaceAgent }),
  /** Costruisce l'avvio di un Bot per il tab `tabId` (il backend ne prepara l'ambiente); rifiuta con il motivo. */
  botInteractiveLaunch: (workspaceId: string, botId: string, tabId: string) =>
    invoke<BotTabLaunch>("bot_interactive_launch", { workspaceId, botId, tabId }),
  /** Rimette l'ambiente di un tab Bot ripristinato (senza ricaricare la skill); l'esito porta l'avviso se riparte senza le scelte del Bot. */
  botRestoreEnv: (workspaceId: string, botId: string, agentId: string, tabId: string) =>
    invoke<BotTabRestore>("bot_restore_env", { workspaceId, botId, agentId, tabId }),

  getLaunchSettings: () => invoke<LaunchSettings>("get_launch_settings"),

  setGlobalDefaultLaunch: (launchId: string | null) =>
    invoke<LaunchSettings>("set_global_default_launch", { launchId }),

  removeLaunch: (workspaceId: string, launchId: string) =>
    invoke<Workspace>("remove_launch", { workspaceId, launchId }),

  openWorkspaceFolder: (id: string) => invoke<void>("open_workspace_folder", { id }),

  listWslDistros: () => invoke<string[]>("list_wsl_distros"),

  checkWslPath: (distro: string, path: string) => invoke<boolean>("check_wsl_path", { distro, path }),

  // Nota: Tauri v2 converte i nomi degli argomenti in camelCase
  // (workspace_id -> workspaceId): le chiavi qui devono esserlo già.
  ptySpawn: (
    workspaceId: string,
    tabId: string,
    cols: number,
    rows: number,
    onEvent: Channel<PtyEvent>,
    cwd?: string | null,
  ) => invoke<number>("pty_spawn", { workspaceId, tabId, cols, rows, onEvent, cwd: cwd ?? null }),

  ptyWrite: (sessionId: number, data: string) => invoke<void>("pty_write", { sessionId, data }),

  ptyResize: (sessionId: number, cols: number, rows: number) => invoke<void>("pty_resize", { sessionId, cols, rows }),

  ptyClose: (sessionId: number) => invoke<void>("pty_close", { sessionId }),

  testAgentNotify: () => invoke<string>("test_agent_notify"),

  ipcStatus: () => invoke<IpcStatus>("ipc_status"),

  /** Toast cliccabile (solo Windows): al click focus + `notification-activated`. */
  showAgentNotification: (tabId: string, title: string, body: string) =>
    invoke<void>("show_agent_notification", { tabId, title, body }),

  /** Toast cliccabile di un thread dell'HUB (solo Windows): al click focus + `hub-notification-activated`. */
  showHubNotification: (channel: string, threadId: string, title: string, body: string) =>
    invoke<void>("show_hub_notification", { channel, threadId, title, body }),

  setShowUsage: (enabled: boolean) => invoke<LaunchSettings>("set_show_usage", { enabled }),

  getBotSettings: () => invoke<BotSettings>("get_bot_settings"),

  setBotSettings: (enabled: boolean, names: string[]) => invoke<BotSettings>("set_bot_settings", { enabled, names }),

  resetBotNames: () => invoke<BotSettings>("reset_bot_names"),

  setBotImage: (name: string, png: string, color: string) => invoke<BotSettings>("set_bot_image", { name, png, color }),

  removeBotImage: (name: string) => invoke<BotSettings>("remove_bot_image", { name }),

  // Bot-agente (backend: src-tauri/src/bots/commands.rs): globali, salvati in bots.json.
  // Le modifiche restituiscono le impostazioni dei bot salvate.
  listAgentBots: () => invoke<AgentBot[]>("list_agent_bots"),

  createAgentBot: (draft: AgentBotDraft) => invoke<BotSettings>("create_agent_bot", { draft }),

  updateAgentBot: (id: string, draft: AgentBotDraft) => invoke<BotSettings>("update_agent_bot", { id, draft }),

  /** Rifiutato per il Bot `general`. */
  deleteAgentBot: (id: string) => invoke<BotSettings>("delete_agent_bot", { id }),

  /** Compito, modello, effort e skill tornano quelli del predefinito; nome e immagine restano. */
  restoreAgentBotPreset: (id: string) => invoke<BotSettings>("restore_agent_bot_preset", { id }),

  /** Ricrea i Bot predefiniti che mancano. */
  restoreAgentBotPresets: () => invoke<BotSettings>("restore_agent_bot_presets"),

  /** Skill selezionabili per un Bot: del plugin, poi dell'utente. */
  listBotSkills: () => invoke<BotSkillOption[]>("list_bot_skills"),

  // Agents HUB (backend: src-tauri/src/hub). `channel` = "general" o l'id di un workspace.
  // Le impostazioni si leggono e scrivono anche con l'HUB spento; ogni comando
  // ritorna quelle salvate con la sola presenza della API key (mai la chiave).
  hubGetSettings: () => invoke<HubSettingsView>("hub_get_settings"),

  hubSetEnabled: (enabled: boolean) => invoke<HubSettingsView>("hub_set_enabled", { enabled }),

  /** Endpoint compatibile OpenAI e modello; stringhe vuote = non configurato. */
  hubSetProvider: (baseUrl: string, model: string) => invoke<HubSettingsView>("hub_set_provider", { baseUrl, model }),

  /** Salva la API key nel portachiavi del sistema. */
  hubSetApiKey: (key: string) => invoke<HubSettingsView>("hub_set_api_key", { key }),

  hubClearApiKey: () => invoke<HubSettingsView>("hub_clear_api_key"),

  hubSetLanguage: (language: HubLanguage) => invoke<HubSettingsView>("hub_set_language", { language }),

  hubSetNotifications: (enabled: boolean) => invoke<HubSettingsView>("hub_set_notifications", { enabled }),

  hubSetSession: (idleMinutes: number, maxSteps: number) =>
    invoke<HubSettingsView>("hub_set_session", { idleMinutes, maxSteps }),

  /** I segretari predefiniti, con i testi per l'interfaccia. */
  hubListSecretaryPresets: () => invoke<HubSecretaryPreset[]>("hub_list_secretary_presets"),

  /** `channels` null = tutti i canali; altrimenti "general" o id di workspace. `preset` = id del preset da cui nasce. */
  hubCreateSecretary: (
    name: string,
    systemPrompt: string,
    tools: HubSecretaryTool[],
    channels: string[] | null,
    preset: string | null,
  ) => invoke<HubSettingsView>("hub_create_secretary", { name, systemPrompt, tools, channels, preset }),

  /** Riporta prompt e tool del segretario a quelli del suo preset; nome, immagine e canali restano. */
  hubRestoreSecretaryPreset: (name: string) => invoke<HubSettingsView>("hub_restore_secretary_preset", { name }),

  /** Sostituisce i campi del segretario `name` (anche il nome); l'immagine resta. */
  hubUpdateSecretary: (
    name: string,
    newName: string,
    systemPrompt: string,
    tools: HubSecretaryTool[],
    channels: string[] | null,
  ) => invoke<HubSettingsView>("hub_update_secretary", { name, newName, systemPrompt, tools, channels }),

  hubDeleteSecretary: (name: string) => invoke<HubSettingsView>("hub_delete_secretary", { name }),

  hubSetSecretaryImage: (name: string, png: string, color: string) =>
    invoke<HubSettingsView>("hub_set_secretary_image", { name, png, color }),

  hubRemoveSecretaryImage: (name: string) => invoke<HubSettingsView>("hub_remove_secretary_image", { name }),

  hubListThreads: (channel: string) => invoke<HubThreadSummary[]>("hub_list_threads", { channel }),

  hubGetThread: (channel: string, threadId: string) => invoke<HubThread>("hub_get_thread", { channel, threadId }),

  hubCreateThread: (channel: string, title: string) => invoke<HubThread>("hub_create_thread", { channel, title }),

  /**
   * Messaggio dell'utente; `replyTo` = id di un messaggio dello stesso thread; `attachments` = id degli allegati
   * già caricati con `hubStageAttachment*` (con almeno uno il testo può essere vuoto).
   */
  hubSend: (
    channel: string,
    threadId: string,
    body: string,
    replyTo: string | null = null,
    attachments: string[] = [],
  ) => invoke<HubMessage>("hub_send", { channel, threadId, body, replyTo, attachments }),

  /**
   * Decide la richiesta di azione di un segretario (la scheda nel thread): `confirm` la esegue,
   * altrimenti la annulla. Una richiesta scaduta o non più valida torna aggiornata (stato e nota)
   * senza errore; una già decisa è un errore.
   */
  hubResolveActionRequest: (channel: string, threadId: string, messageId: string, confirm: boolean) =>
    invoke<HubActionRequest>("hub_resolve_action_request", { channel, threadId, messageId, confirm }),

  /** Chiude (`closed`) o riapre il thread; riaprire non sveglia nessun segretario. Torna il thread aggiornato. */
  hubSetThreadState: (channel: string, threadId: string, closed: boolean) =>
    invoke<HubThreadSummary>("hub_set_thread_state", { channel, threadId, closed }),

  /** Elimina il thread (aperto o chiuso) e i suoi allegati; i task collegati restano sulla board. */
  hubDeleteThread: (channel: string, threadId: string) => invoke<void>("hub_delete_thread", { channel, threadId }),

  /** Carica nello staging il file scelto con il dialog (percorso assoluto); poi l'id va a `hubSend`. */
  hubStageAttachmentFile: (path: string) => invoke<HubAttachment>("hub_stage_attachment_file", { path }),

  /** Carica nello staging i byte di un file senza percorso (immagine incollata, file trascinato). */
  hubStageAttachmentBytes: (name: string, bytes: Uint8Array) =>
    invoke<HubAttachment>("hub_stage_attachment_bytes", bytes, {
      headers: { [HUB_FILE_NAME_HEADER]: encodeURIComponent(name) },
    }),

  /** I byte di un allegato (per l'anteprima con un Blob URL). */
  hubReadAttachment: (channel: string, threadId: string, attachmentId: string) =>
    invoke<ArrayBuffer>("hub_read_attachment", { channel, threadId, attachmentId }),

  /** Percorso assoluto di un allegato (per aprirlo con l'app predefinita). */
  hubAttachmentPath: (channel: string, threadId: string, attachmentId: string) =>
    invoke<string>("hub_attachment_path", { channel, threadId, attachmentId }),

  /** Segretari svegli adesso, per canale e thread (poi `hub-secretary-status`). */
  hubSecretaryStatus: () => invoke<HubSecretaryStatus[]>("hub_secretary_status"),

  // Connettori dell'Agents HUB (backend: src-tauri/src/connectors/commands). I segreti
  // passano solo da `connectorSetSecret` e non tornano mai: si vede se uno slot è impostato.
  connectorListDescriptors: () => invoke<ConnectorDescriptor[]>("connector_list_descriptors"),

  connectorListInstances: () => invoke<ConnectorInstanceView[]>("connector_list_instances"),

  /** Lo stato di salute di tutte le istanze (poi `connector-status`). */
  connectorStatus: () => invoke<ConnectorStatus[]>("connector_status"),

  /** Crea un'istanza disabilitata; i segreti si impostano dopo. */
  connectorCreateInstance: (connector: string, label: string, config: Record<string, unknown>) =>
    invoke<ConnectorInstanceView>("connector_create_instance", { connector, label, config }),

  connectorUpdateInstance: (id: string, label: string, config: Record<string, unknown>) =>
    invoke<ConnectorInstanceView>("connector_update_instance", { id, label, config }),

  connectorDeleteInstance: (id: string) => invoke<ConnectorDeleteOutcome>("connector_delete_instance", { id }),

  connectorSetEnabled: (id: string, enabled: boolean) =>
    invoke<ConnectorInstanceView>("connector_set_enabled", { id, enabled }),

  connectorSetSecret: (id: string, slot: string, value: string) =>
    invoke<ConnectorInstanceView>("connector_set_secret", { id, slot, value }),

  connectorClearSecret: (id: string, slot: string) =>
    invoke<ConnectorInstanceView>("connector_clear_secret", { id, slot }),

  /** Prova di credenziali e rete (azione `test`, se il connettore la dichiara). */
  connectorTest: (id: string) => invoke<ConnectorActionOutcome>("connector_test", { id }),

  /** Un'azione dichiarata dal connettore; con `pair` l'esito porta il codice di abbinamento. */
  connectorRunAction: (id: string, action: ConnectorActionId) =>
    invoke<ConnectorActionOutcome>("connector_run_action", { id, action }),

  /** Le regole, filtrabili per canale e per thread (quelle che puntano a quel thread). */
  connectorListRoutes: (channel: string | null, threadId: string | null) =>
    invoke<ConnectorRoute[]>("connector_list_routes", { channel, threadId }),

  connectorCreateRoute: (instance: string, route: ConnectorRouteInput) =>
    invoke<ConnectorRoute>("connector_create_route", { instance, route }),

  connectorUpdateRoute: (id: string, route: ConnectorRouteInput) =>
    invoke<ConnectorRoute>("connector_update_route", { id, route }),

  connectorDeleteRoute: (id: string) => invoke<void>("connector_delete_route", { id }),

  /** Gli account esterni collegati a un'istanza di contatto. */
  connectorListLinks: (instance: string) => invoke<ConnectorLink[]>("connector_list_links", { instance }),

  connectorDeleteLink: (id: string) => invoke<void>("connector_delete_link", { id }),

  agentUsageLimits: (agentId: string, env: string) => invoke<UsageLimitsReply>("agent_usage_limits", { agentId, env }),

  agentSessionUsage: (agentId: string, env: string, transcriptPath: string) =>
    invoke<SessionUsage | null>("agent_session_usage", { agentId, env, transcriptPath }),

  agentProfiles: () => invoke<AgentProfile[]>("agent_profiles"),

  agentEnvStatus: () => invoke<AgentEnvStatus[]>("agent_env_status"),

  installAgentPlugin: (agentId: string, envKey: string) => invoke<string>("install_agent_plugin", { agentId, envKey }),

  agentPluginsOutdated: () => invoke<OutdatedPlugin[]>("agent_plugins_outdated"),

  agentPluginsUpdate: () => invoke<PluginUpdateResult[]>("agent_plugins_update"),

  removeAgentPlugin: (agentId: string, envKey: string) => invoke<string>("remove_agent_plugin", { agentId, envKey }),

  /** Tutte le integrazioni con stato e prontezza per target (backend: agents/integrations_commands.rs). */
  integrationsList: () => invoke<IntegrationInfo[]>("integrations_list"),

  setIntegrationEnabled: (id: string, enabled: boolean) =>
    invoke<IntegrationsState>("set_integration_enabled", { id, enabled }),

  setDefaultIntegration: (id: string) => invoke<IntegrationsState>("set_default_integration", { id }),

  // Sessioni automatiche (backend: src-tauri/src/automation/).
  /** Un tab automatico e' stato chiuso (utente, app o shell uscita). */
  automationTabClosed: (tabId: string) => invoke<void>("automation_tab_closed", { tabId }),

  automationSessions: () => invoke<AutomationTabInfo[]>("automation_sessions"),

  /** Listener registrati e sessione ripristinata: il timer dell'automazione puo' partire (idempotente). */
  frontendReady: () => invoke<void>("frontend_ready"),

  // Coda dei task (backend: src-tauri, contratto in docs/BOARD.MD).
  boardGet: (workspaceId: string) => invoke<BoardView>("board_get", { workspaceId }),

  /** Avvia a mano gli E2E di un repository ("." = root); ritorna l'esecuzione in corso. */
  e2eRunRepo: (workspaceId: string, repo: string) => invoke<E2eRun>("e2e_run_repo", { workspaceId, repo }),

  /** Ultimo esito manuale degli E2E di un repository, se c'è. */
  e2eLastResult: (workspaceId: string, repo: string) => invoke<E2eRun | null>("e2e_last_result", { workspaceId, repo }),

  /** Apre il report JUnit dell'ultima esecuzione E2E del task, o senza `taskId` di quella manuale del repository. */
  e2eOpenReport: (workspaceId: string, taskId?: string, repo?: string) =>
    invoke<void>("e2e_open_report", { workspaceId, taskId, repo }),

  /** Repository del workspace: "." se la root è un repo, più le sottocartelle con un .git. */
  workspaceDetectRepos: (workspaceId: string) => invoke<string[]>("workspace_detect_repos", { workspaceId }),

  boardSet: (workspaceId: string, settings: { maxParallel?: number; paused?: boolean }) =>
    invoke<BoardView>("board_set", {
      workspaceId,
      maxParallel: settings.maxParallel ?? null,
      paused: settings.paused ?? null,
    }),

  /** Task manuale (senza proposta); l'id lo genera il backend dal titolo. */
  taskCreate: (workspaceId: string, spec: NewTaskSpec) => invoke<BoardView>("task_create", { workspaceId, spec }),

  /** Solo i task Da fare / Falliti; `repo` e `model` a null tornano al valore predefinito. */
  taskUpdate: (workspaceId: string, taskId: string, patch: TaskPatch) =>
    invoke<BoardView>("task_update", { workspaceId, taskId, patch }),

  taskAction: (workspaceId: string, taskId: string, action: TaskAction) =>
    invoke<BoardView>("task_action", { workspaceId, taskId, action }),

  /** Riordina un task Da fare: prima di `beforeId`, o in fondo se assente. */
  taskMove: (workspaceId: string, taskId: string, beforeId?: string | null) =>
    invoke<BoardView>("task_move", { workspaceId, taskId, beforeId: beforeId ?? null }),

  /**
   * Accetta o scarta una proposta di Task Intake. `autoDeliver` e `autoFix` (giri di correzione automatica, 0..=3)
   * sono le scelte dell'utente per il goal nuovo che la proposta crea; null altrimenti.
   */
  proposalDecide: (
    workspaceId: string,
    proposalId: string,
    accept: boolean,
    autoDeliver: boolean | null,
    autoFix: number | null,
  ) => invoke<BoardView>("proposal_decide", { workspaceId, proposalId, accept, autoDeliver, autoFix }),

  /** Giri di correzione automatica di un goal aperto (0..=3). */
  goalSetAutoFix: (workspaceId: string, goalId: string, maxRounds: number) =>
    invoke<BoardView>("goal_set_auto_fix", { workspaceId, goalId, maxRounds }),

  /** "Consegna" / "Consegna comunque": crea il task di consegna del goal pronto. */
  goalDeliver: (workspaceId: string, goalId: string) => invoke<BoardView>("goal_deliver", { workspaceId, goalId }),

  /** Impostazioni di consegna del goal: consegna automatica e Bot (null = invariato). */
  goalSetDelivery: (workspaceId: string, goalId: string, autoDeliver: boolean | null, bot: string | null) =>
    invoke<BoardView>("goal_set_delivery", { workspaceId, goalId, autoDeliver, bot }),

  /** "Chiudi goal": elimina i task in coda e falliti del goal, scarta le sue proposte e lo chiude. */
  goalClose: (workspaceId: string, goalId: string) => invoke<BoardView>("goal_close", { workspaceId, goalId }),

  /** "Elimina branch": cancella i branch `goal/<id>` di un goal chiuso senza consegna e li dimentica. */
  goalDeleteBranches: (workspaceId: string, goalId: string) =>
    invoke<BoardView>("goal_delete_branches", { workspaceId, goalId }),

  /** Nota dell'utente nel registro delle decisioni di un goal aperto. */
  goalAddNote: (workspaceId: string, goalId: string, text: string) =>
    invoke<BoardView>("goal_add_note", { workspaceId, goalId, text }),

  /** Applica o scarta una proposta di modifica alle impostazioni. */
  settingsProposalDecide: (workspaceId: string, proposalId: string, accept: boolean) =>
    invoke<SettingsDecision>("settings_proposal_decide", { workspaceId, proposalId, accept }),

  // Avvisi di persistenza (backend: src-tauri/src/store/warnings.rs); evento "store-warning".
  storeWarnings: () => invoke<StoreWarning[]>("store_warnings"),

  // Fasce orarie globali (solo come gate dello scheduler, docs/BOARD.MD).
  scheduleGet: () => invoke<ScheduleState>("schedule_get"),

  scheduleSet: (schedule: Schedule) => invoke<ScheduleState>("schedule_set", { schedule }),

  // Persistenza della sessione (backend: src-tauri/src/session.rs).
  loadSession: () => invoke<string | null>("load_session"),

  saveSession: (data: string) => invoke<void>("save_session", { data }),

  saveScrollback: (tabId: string, data: string) => invoke<void>("save_scrollback", { tabId, data }),

  loadScrollback: (tabId: string) => invoke<string | null>("load_scrollback", { tabId }),

  deleteScrollback: (tabId: string) => invoke<void>("delete_scrollback", { tabId }),

  pruneScrollback: (keep: string[]) => invoke<void>("prune_scrollback", { keep }),
};

/** Eventi emessi dal backend con il tipo del loro payload (usati da `useTauriEvent`). */
export interface TauriEvents {
  "notification-activated": AutomationClose;
  "automation-spawn": AutomationSpawn;
  "automation-close": AutomationClose;
  "automation-update": AutomationTabInfo;
  "schedule-state": ScheduleState;
  "board-update": BoardView;
  "e2e-update": E2eUpdateEvent;
  "task-notify": TaskNotifyEvent;
  "agent-event": AgentEvent;
  "integrations-changed": IntegrationsState;
  "store-warning": StoreWarning;
  "hub-update": HubThreadSummary;
  "hub-notify": HubNotice;
  "hub-notification-activated": HubThreadRef;
  "hub-thread-removed": HubThreadRef;
  "hub-secretary-status": HubSecretaryStatus[];
  "connector-status": ConnectorStatus[];
}
