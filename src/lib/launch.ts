// Avvii personalizzati per workspace: comando eseguito all'apertura di un
// terminale. File volutamente a zero dipendenze: importabile sia dall'app sia
// dai test `node --test` (type-stripping), come src/lib/terminal.ts.

/** Avvio di un workspace (rispecchia `Launch` di workspaces.rs). */
export interface LaunchLike {
  id: string;
  name: string;
  command: string;
  is_default?: boolean;
}

/** Lunghezza massima del comando di un avvio. */
export const LAUNCH_COMMAND_MAX = 1000;

/** Lunghezza massima del nome di un avvio. */
export const LAUNCH_NAME_MAX = 80;

/** Attesa dopo il primo output del pty prima di inviare il comando. */
export const LAUNCH_AFTER_OUTPUT_MS = 150;

/**
 * Fallback: se la shell non ha ancora stampato nulla (avvio freddo WSL),
 * invia comunque il comando dopo questo tempo dallo spawn.
 */
export const LAUNCH_FALLBACK_MS = 2500;

/**
 * Normalizza il comando: trim, stringa vuota se blank, troncato a 1000
 * caratteri. Lo spacing interno è preservato (fa parte del comando).
 */
export function sanitizeLaunchCommand(raw: string): string {
  const trimmed = raw.trim();
  if (trimmed.length <= LAUNCH_COMMAND_MAX) return trimmed;
  return trimmed.slice(0, LAUNCH_COMMAND_MAX);
}

/** True solo se c'è un comando da eseguire (non vuoto/blank). */
export function shouldInjectLaunch(cmd: string): boolean {
  return cmd.trim() !== "";
}

/**
 * Costruisce l'input da scrivere nel pty: comando + ritorno carrello
 * (Invio). Funziona su PowerShell/cmd e sulle shell WSL perché simula
 * la digitazione dell'utente.
 */
export function buildLaunchInput(cmd: string): string {
  return `${cmd}\r`;
}

/** Avvio predefinito del workspace, se ce n'è uno. */
export function defaultLaunch<T extends LaunchLike>(ws: {
  launches?: T[];
}): T | undefined {
  return ws.launches?.find((l) => l.is_default);
}
