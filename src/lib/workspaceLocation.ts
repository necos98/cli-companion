// Posizione di un workspace scelta nei dialoghi (ambiente, distro, percorso):
// i campi del modulo, quelli di partenza e la verifica prima dell'invio.
// Zero-dep, testato da tests/workspace-location.test.mjs.

import type { TargetKind } from "../bindings/TargetKind.ts";
import type { Workspace } from "../bindings/Workspace.ts";

/** I campi del modulo: un percorso per ambiente, così passare da uno all'altro non perde quanto scritto. */
export interface LocationFields {
  kind: TargetKind;
  winPath: string;
  distro: string;
  wslPath: string;
}

/** Posizione da inviare al backend (`add_workspace`, `set_workspace_location`). */
export interface ChosenLocation {
  kind: TargetKind;
  path: string;
  distro?: string;
}

export type LocationChoice = { ok: true; location: ChosenLocation } | { ok: false; error: string };

export const EMPTY_LOCATION: LocationFields = { kind: "windows", winPath: "", distro: "", wslPath: "" };

/** I campi precompilati con la posizione attuale del workspace. */
export function locationOf(ws: Pick<Workspace, "kind" | "path" | "distro">): LocationFields {
  return ws.kind === "wsl"
    ? { ...EMPTY_LOCATION, kind: "wsl", distro: ws.distro ?? "", wslPath: ws.path }
    : { ...EMPTY_LOCATION, kind: "windows", winPath: ws.path };
}

/**
 * La posizione da inviare, o l'errore da mostrare se manca qualcosa. Il percorso WSL arriva già
 * convertito in Linux (`toWslPath`), con la distro che eventualmente nomina.
 */
export function chooseLocation(fields: LocationFields): LocationChoice {
  if (fields.kind === "windows") {
    const path = fields.winPath.trim();
    return path ? { ok: true, location: { kind: "windows", path } } : { ok: false, error: "Seleziona una cartella." };
  }
  const path = fields.wslPath.trim();
  if (!path) return { ok: false, error: "Inserisci il percorso Linux." };
  if (!fields.distro) return { ok: false, error: "Seleziona una distro WSL." };
  return { ok: true, location: { kind: "wsl", path, distro: fields.distro } };
}

/** La scelta cambia l'ambiente del workspace: la sua shell personalizzata torna quella predefinita. */
export function resetsShell(ws: Pick<Workspace, "kind" | "shell">, kind: TargetKind): boolean {
  return ws.kind !== kind && !!ws.shell;
}
