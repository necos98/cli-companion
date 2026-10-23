import { openUrl } from "@tauri-apps/plugin-opener";

/** Apre un indirizzo http(s) nel browser di sistema; fuori da Tauri ripiega su una nuova scheda. */
export function openExternal(url: string): void {
  openUrl(url).catch(() => {
    try {
      window.open(url, "_blank", "noopener,noreferrer");
    } catch {
      // Nessun modo di aprirlo: l'utente può copiare l'indirizzo dal testo.
    }
  });
}
