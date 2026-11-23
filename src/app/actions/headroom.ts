// Headroom: impostazioni globali (interruttore e porta) e dashboard. Lo stato
// sta in `stores/headroom.ts`; qui l'I/O col backend.

import { api } from "../../lib/tauri";
import type { HeadroomSettings } from "../../bindings/HeadroomSettings";
import { report } from "../../lib/errors";
import { headroomStore } from "../../stores/headroom";

/** Impostazioni lette all'avvio; l'errore è già segnalato. */
export function loadHeadroomSettings(): Promise<void> {
  return api
    .headroomSettingsGet()
    .then((s) => headroomStore.settingsLoaded(s))
    .catch((e) => report("caricamento delle impostazioni di Headroom", e, { notify: true }));
}

/** Salva le impostazioni; rifiuta con l'errore del backend (es. porta non valida). */
export async function saveHeadroomSettings(settings: HeadroomSettings): Promise<void> {
  headroomStore.settingsLoaded(await api.headroomSettingsSet(settings));
}

/** Apre la dashboard di Headroom nella sua finestra (o la porta in primo piano). */
export function openHeadroomDashboard(): void {
  api.headroomOpenDashboard().catch((e) => report("apertura della dashboard di Headroom", e, { notify: true }));
}
