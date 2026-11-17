// Gestione dei workspace: aggiunta, modifica, rinomina, cambio di cartella, rimozione,
// archiviazione e riordino. Backend per primo, poi gli archivi.

import { api } from "../../lib/tauri";
import type { Workspace } from "../../lib/tauri";
import type { ChosenLocation } from "../../lib/workspaceLocation";
import { hasGroup, layoutStore, viewWorkspaceId } from "../../stores/layout";
import { tabsStore } from "../../stores/tabs";
import { workspacesStore } from "../../stores/workspaces";
import { closeTab } from "./tabs";
import { selectWorkspace } from "./view";

export function workspaceAdded(ws: Workspace): void {
  workspacesStore.added(ws);
}

/**
 * Aggiorna lo stato con il Workspace restituito dal backend (rinomina,
 * modifica degli avvii): i tab già aperti lo leggono dall'elenco.
 */
export function workspaceUpdated(updated: Workspace): void {
  workspacesStore.updated(updated);
}

export async function renameWorkspace(id: string, name: string): Promise<void> {
  workspaceUpdated(await api.renameWorkspace(id, name));
}

/** Collega il workspace a un'altra cartella: i tab aperti restano dove sono, i nuovi partono da lì. */
export async function changeWorkspaceLocation(id: string, location: ChosenLocation): Promise<void> {
  workspaceUpdated(await api.setWorkspaceLocation(id, location));
}

/** Chiude i tab di un workspace, con la stessa via della chiusura da parte dell'utente. */
function closeTabsOf(workspaceId: string): void {
  for (const tab of tabsStore.getState()) {
    if (tab.workspaceId === workspaceId) closeTab(tab.key);
  }
}

export async function removeWorkspace(id: string): Promise<void> {
  await api.removeWorkspace(id);
  // Stessa via della chiusura da parte dell'utente: pty (smontaggio del
  // TerminalView), scrollback, gruppo e selezione di un altro gruppo.
  closeTabsOf(id);
  workspacesStore.removed(id);
}

export async function reorderWorkspaces(ids: string[]): Promise<void> {
  workspacesStore.reordered(await api.reorderWorkspaces(ids));
}

export async function archiveWorkspace(id: string): Promise<void> {
  // Prima il backend (la coda si ferma), poi la chiusura dei terminali.
  workspaceUpdated(await api.setWorkspaceArchived(id, true));
  // Si guarda se era quello mostrato PRIMA di chiudere i tab: chiuderli sposta
  // già la selezione sul gruppo vicino, mentre qui si sceglie il primo con terminali.
  const state = layoutStore.getState();
  const wasShown = viewWorkspaceId(state) === id || state.layout.activeWorkspaceId === id;
  closeTabsOf(id);
  if (!wasShown) return;
  const others = workspacesStore.getState().list.filter((w) => !w.archived && w.id !== id);
  const { layout } = layoutStore.getState();
  const next = others.find((w) => hasGroup(layout, w.id)) ?? others[0];
  layoutStore.focusWorkspace(null);
  if (next) selectWorkspace(next.id);
}
