// Notifica di sistema Windows (plugin ufficiale tauri-plugin-notification).
// Fuori da agents.ts perche' importa Tauri: i test node non lo caricano.

import { api } from "./tauri";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

/** `true` se la finestra dell'app ha il focus (altrimenti: notifica di sistema). */
async function appHasFocus(): Promise<boolean> {
  try {
    return await getCurrentWindow().isFocused();
  } catch {
    return document.hasFocus();
  }
}

/**
 * Notifica di sistema senza tab associato (nessun click: il toast cliccabile
 * porta a un tab). Mostrata solo se la finestra non e' in primo piano.
 */
export async function notifyPlainIfUnfocused(title: string, body: string): Promise<void> {
  try {
    if (await appHasFocus()) return;
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (granted) sendNotification({ title, body });
  } catch {
    // Notifiche non disponibili.
  }
}

/**
 * Mostra una notifica di sistema se la finestra non e' in primo piano.
 * Sicura da chiamare ovunque: permessi negati o plugin assente = nessun errore.
 */
export async function notifyIfUnfocused(
  tabId: string,
  title: string,
  body: string,
): Promise<void> {
  try {
    if (await appHasFocus()) return;
    try {
      // Toast cliccabile (Windows): il click porta al tab. Altrove errore.
      await api.showAgentNotification(tabId, title, body);
      return;
    } catch {
      // Non disponibile: plugin ufficiale, senza click.
    }
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (granted) sendNotification({ title, body });
  } catch {
    // Notifiche non disponibili: il suono resta come segnale.
  }
}
