// Operazioni della sidebar sui workspace: l'errore o l'esito arrivano nello
// stato condiviso (`sidebarStore`), da cui la lista mostra gli errori.

import { api } from "../../lib/tauri";
import type { Workspace } from "../../lib/tauri";
import { openLaunchSettings, openWorkspaceLocation } from "../../app/actions/settings";
import { archiveWorkspace, removeWorkspace, renameWorkspace } from "../../app/actions/workspaces";
import { sidebarStore } from "./sidebarState";

/** Esegue `task` con il workspace segnato come occupato; in caso di errore lo mostra, altrimenti chiama `done`. */
async function runBusy(id: string, task: () => Promise<void>, done: () => void): Promise<void> {
  sidebarStore.beginBusy(id);
  try {
    await task();
    done();
  } catch (e) {
    sidebarStore.setError(String(e));
  } finally {
    sidebarStore.endBusy();
  }
}

export function saveEdit(id: string): Promise<void> {
  return runBusy(
    id,
    () => renameWorkspace(id, sidebarStore.getState().editingName),
    () => sidebarStore.stopEdit(),
  );
}

export function confirmRemove(id: string): Promise<void> {
  return runBusy(
    id,
    () => removeWorkspace(id),
    () => sidebarStore.cancelRemove(),
  );
}

export function confirmArchive(id: string): Promise<void> {
  return runBusy(
    id,
    () => archiveWorkspace(id),
    () => sidebarStore.cancelArchive(),
  );
}

export function openFolder(id: string): void {
  sidebarStore.setError(null);
  api.openWorkspaceFolder(id).catch((e) => sidebarStore.setError(String(e)));
}

// Il dialog delle impostazioni si apre solo a menu chiuso, così i due layer
// Radix non si sovrappongono (altrimenti resta `pointer-events: none`).
export function manageLaunches(ws: Workspace): void {
  setTimeout(() => openLaunchSettings(ws), 0);
}

/** "Cambia cartella…": stesso motivo, il dialogo si apre a menu chiuso. */
export function changeLocation(ws: Workspace): void {
  setTimeout(() => openWorkspaceLocation(ws), 0);
}
