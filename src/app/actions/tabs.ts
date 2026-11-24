// Apertura e chiusura dei terminali, e ciò che i TerminalView riportano
// (titolo, input, stato). Funzioni di modulo (stabili): leggono lo stato
// dagli archivi con `getState()`.

import { api } from "../../lib/tauri";
import type { BotLaunch, BuiltinLaunch, Launch, Workspace } from "../../lib/tauri";
import { botTabSpec, botWarningsText, isBotLaunch } from "../../lib/botLaunch";
import { errorMessage, report } from "../../lib/errors";
import { isIntakeLaunch } from "../../lib/automation";
import { launchAgentId, launchCommands, unavailableDefaultLaunch } from "../../lib/launch";
import { launchBlockReason } from "../../lib/launchMenu";
import { noticeStore } from "../../lib/notices";
import { workspaceEnvKey } from "../../lib/usage";
import { closeTabIn, openTab } from "../../lib/layout";
import { botLaunchesFor, configStore, defaultLaunchFor, sharedLaunchesFor } from "../../stores/config";
import { hubStore, reservedBotNames } from "../../stores/hub";
import { layoutStore } from "../../stores/layout";
import { paneDragStore } from "../../stores/paneDrag";
import { runtimeStore } from "../../stores/tabRuntime";
import { tabsStore } from "../../stores/tabs";
import { workspacesStore } from "../../stores/workspaces";
import { isClosing } from "../lifecycle";
import { openIntegrationSettings } from "./settings";
import { showTerminalsOf } from "./view";

export function newTabKey(): string {
  try {
    return crypto.randomUUID();
  } catch {
    return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  }
}

/** Avviso (non un errore) mostrato sopra anche alle Impostazioni. */
function warn(key: string, text: string): void {
  noticeStore.push({ key, level: "warning", text });
}

/**
 * Senza `launch` parte l'avvio predefinito del workspace, se esiste (un Bot
 * passa dal backend come dal menu); altrimenti una shell semplice, con un avviso
 * se il predefinito salvato punta a un avvio non più disponibile. Il nuovo
 * terminale attiva il suo gruppo.
 */
export function spawnTab(ws: Workspace, launch?: Launch | BuiltinLaunch): void {
  const config = configStore.getState();
  const chosen = launch ?? defaultLaunchFor(config, ws);
  if (chosen && isBotLaunch(chosen)) {
    void spawnBotFromMenu(ws, chosen);
    return;
  }
  if (!launch && unavailableDefaultLaunch(ws, sharedLaunchesFor(config, ws), config.globalLaunch) !== null) {
    warn(
      "launch:default-unavailable",
      "L'avvio predefinito non è più disponibile (integrazione disabilitata o Bot eliminato): apro una shell vuota.",
    );
  }
  const key = newTabKey();
  if (launchAgentId(chosen) !== undefined) {
    void spawnAgentTab(ws, chosen, key);
    return;
  }
  openLaunchTab(ws, chosen, key);
}

/**
 * Avvio integrato di un agente: prima il backend prepara l'ambiente del tab
 * (es. il proxy Headroom per Claude Code e Pi), che `pty_spawn` applica una volta
 * sola; se non risponde il tab si apre comunque, senza.
 */
async function spawnAgentTab(ws: Workspace, chosen: Launch | BuiltinLaunch | undefined, key: string): Promise<void> {
  try {
    const warning = await api.headroomStageEnv(ws.id, launchAgentId(chosen)!, key);
    if (warning !== null) warn("launch:headroom", warning);
  } catch (e) {
    report("preparazione dell'ambiente Headroom", e);
  }
  openLaunchTab(ws, chosen, key);
}

function openLaunchTab(ws: Workspace, chosen: Launch | BuiltinLaunch | undefined, key: string): void {
  const config = configStore.getState();
  // Per gli integrati si salva il comando di ripristino, si esegue quello di avvio.
  const { command, startCommand } = launchCommands(chosen);
  showTerminalsOf(ws.id);
  runtimeStore.seed(key);
  // Apertura sincrona: due tab aperti di seguito non prendono lo stesso nome.
  tabsStore.open(
    { key, workspaceId: ws.id, launchId: chosen?.id, agentId: launchAgentId(chosen), command, startCommand },
    config.botSettings.names,
    reservedBotNames(hubStore.getState()),
  );
  layoutStore.apply((l) => openTab(l, ws.id, key));
}

/**
 * Avvio scelto da un menu: se l'integrazione non è pronta nell'ambiente del
 * workspace non parte nulla e si va alle Integrazioni, con il motivo.
 */
export function spawnFromMenu(ws: Workspace, launch: Launch | BuiltinLaunch): void {
  const reason =
    "agent_id" in launch ? launchBlockReason(launch, configStore.getState().integrations, workspaceEnvKey(ws)) : null;
  if (reason === null) {
    spawnTab(ws, launch);
    return;
  }
  warn("launch:not-ready", reason);
  // A menu chiuso, così i due layer Radix non si sovrappongono.
  setTimeout(openIntegrationSettings, 0);
}

/**
 * Bot agente scelto da un menu: il backend costruisce il comando e prepara
 * l'ambiente del tab, che si apre solo se l'avvio riesce. Il tab prende il nome
 * del Bot (con suffisso se già in uso), mai quello di una segretaria.
 */
export async function spawnBotFromMenu(ws: Workspace, entry: BotLaunch): Promise<void> {
  const readiness =
    entry.agent_id === null
      ? null
      : launchBlockReason({ agent_id: entry.agent_id }, configStore.getState().integrations, workspaceEnvKey(ws));
  const reason = entry.blocked_reason ?? readiness;
  if (reason !== null) {
    warn("launch:not-ready", reason);
    // Solo la prontezza si risolve dalle Integrazioni, a menu chiuso (layer Radix).
    if (entry.blocked_reason === null) setTimeout(openIntegrationSettings, 0);
    return;
  }
  const key = newTabKey();
  let launch;
  try {
    launch = await api.botInteractiveLaunch(ws.id, entry.bot_id, key);
  } catch (e) {
    warn("launch:bot-failed", errorMessage(e));
    return;
  }
  const warnings = botWarningsText(launch);
  if (warnings !== null) warn("launch:bot-warnings", warnings);
  showTerminalsOf(ws.id);
  runtimeStore.seed(key);
  // La pool è il solo nome del Bot: `open` aggiunge il suffisso sui doppioni.
  tabsStore.open(botTabSpec(ws.id, key, entry, launch), [launch.name], reservedBotNames(hubStore.getState()));
  layoutStore.apply((l) => openTab(l, ws.id, key));
}

/**
 * Task Intake: ogni richiesta apre un nuovo tab del primo Bot abilitato con la
 * skill Task Intake (più chat in parallelo), con la stessa via del menu. Senza
 * un Bot così avvisa. Ritorna se l'avvio è partito.
 */
export function openIntake(workspaceId: string): boolean {
  const ws = workspacesStore.get(workspaceId);
  if (!ws) return false;
  const config = configStore.getState();
  const entry = botLaunchesFor(config, ws).find((b) => isIntakeLaunch(b.id, config.botSettings.agents));
  if (!entry) {
    warn(
      "launch:intake-unavailable",
      "Task Intake non è disponibile: nessun Bot attivo ha la skill Task Intake (ripristinalo nelle impostazioni dei Bot).",
    );
    return false;
  }
  void spawnBotFromMenu(ws, entry);
  return true;
}

// Un tab automatico chiuso va comunicato al backend una volta sola, da
// qualunque via arrivi la chiusura (utente, `automation-close`, shell uscita).
const reportedClosed = new Set<string>();

/**
 * Chiusura di un terminale (dall'utente o perché la shell è uscita): la sua
 * cronologia salvata non serve più.
 */
export function closeTab(key: string): void {
  const automated = tabsStore.get(key)?.automated === true;
  if (automated && !reportedClosed.has(key)) {
    reportedClosed.add(key);
    void api.automationTabClosed(key).catch((e) => report("chiusura del tab automatico", e));
  }
  tabsStore.remove(key);
  layoutStore.apply((l) => closeTabIn(l, key));
  runtimeStore.remove(key);
  // Un trascinamento con il tab chiuso come origine o bersaglio non può più finire.
  const { from, over } = paneDragStore.getState();
  if (from === key || over === key) paneDragStore.end();
  // Gli automatici non salvano lo scrollback: niente da cancellare.
  if (!automated) void api.deleteScrollback(key).catch((e) => report("eliminazione della cronologia salvata", e));
}

/**
 * La shell è uscita da sola. Durante la chiusura dell'app i pty vengono
 * chiusi: non è un'uscita dell'utente, la sessione va conservata.
 */
export function handleExit(key: string): void {
  if (isClosing()) return;
  closeTab(key);
}

export function handleTitle(key: string, title: string): void {
  runtimeStore.setTitle(key, title);
}

/** Input dell'utente: Invio => l'agente lavora, altro input toglie ✓/❗. */
export function handleInput(key: string, enter: boolean): void {
  runtimeStore.userInput(key, enter);
}

export function handleStatus(key: string, busy: boolean): void {
  runtimeStore.setBusy(key, busy);
}
