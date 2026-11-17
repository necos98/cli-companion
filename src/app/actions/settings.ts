// Dialoghi e impostazioni: apertura di Impostazioni, "Aggiungi workspace" e "Cambia cartella",
// e i valori che le Impostazioni riportano all'app.

import { api } from "../../lib/tauri";
import type { BotSettings, Workspace } from "../../lib/tauri";
import { report } from "../../lib/errors";
import { refreshIntegrations } from "./integrations";
import { configStore } from "../../stores/config";
import { uiStore } from "../../stores/ui";
import { workspacesStore } from "../../stores/workspaces";

/** Impostazioni: sulla pagina "Aggiornamenti" se ce n'è uno disponibile (pallino acceso). */
export function openSettings(): void {
  uiStore.openSettings(uiStore.getState().updateAvailable ? "updates" : "general-launches");
}

/** Impostazioni sulla pagina Integrazioni. */
export function openIntegrationSettings(): void {
  uiStore.openSettings("integrations");
}

/** Impostazioni sulla pagina Agents HUB (modello e segretari). */
export function openHubSettings(): void {
  uiStore.openSettings("hub");
}

/** Impostazioni sulla pagina Connettori. */
export function openConnectorSettings(): void {
  uiStore.openSettings("connectors");
}

/** Impostazioni sugli avvii del workspace. */
export function openLaunchSettings(ws: Workspace): void {
  uiStore.openSettings("launches", ws.id);
}

export function closeSettings(): void {
  uiStore.closeSettings();
}

export function openAddWorkspace(): void {
  uiStore.setAddWorkspaceOpen(true);
}

export function closeAddWorkspace(): void {
  uiStore.setAddWorkspaceOpen(false);
}

/** Dialogo "Cambia cartella" del workspace. */
export function openWorkspaceLocation(ws: Workspace): void {
  uiStore.setLocationWorkspace(ws.id);
}

export function closeWorkspaceLocation(): void {
  uiStore.setLocationWorkspace(null);
}

export function setShowUsage(showUsage: boolean): void {
  configStore.patch({ showUsage });
}

export function setGlobalLaunch(globalLaunch: string | null): void {
  configStore.patch({ globalLaunch });
}

export function setBotSettings(botSettings: BotSettings): void {
  configStore.patch({ botSettings });
}

/**
 * Rilegge workspace e avvio predefinito globale: eliminare un Bot azzera i
 * predefiniti che lo usavano, e i menu non devono mostrarne uno fantasma.
 */
export async function reloadLaunchDefaults(): Promise<void> {
  try {
    const [list, settings] = await Promise.all([api.listWorkspaces(), api.getLaunchSettings()]);
    workspacesStore.loaded(list);
    configStore.patch({ globalLaunch: settings.default_launch });
  } catch (e) {
    report("aggiornamento degli avvii predefiniti", e, { notify: true });
  }
}

export function closePluginUpdate(): void {
  uiStore.setOutdatedPlugins([]);
  // Dopo un aggiornamento non seguito da riavvio lo stato va riletto.
  refreshIntegrations().catch((e) => report("rilevamento dei pacchetti installati", e));
}
