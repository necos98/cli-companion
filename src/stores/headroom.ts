// Headroom (proxy locale di compressione per Claude Code): impostazioni
// globali. Funzioni pure + archivio, come `stores/ui.ts`.

import { createStore } from "../lib/store.ts";
import type { HeadroomSettings } from "../bindings/HeadroomSettings.ts";

export interface HeadroomState {
  settings: HeadroomSettings;
}

export const INITIAL_HEADROOM: HeadroomState = {
  settings: { enabled: false, port: 8787, auto_start: true, executable: "" },
};

export function createHeadroomStore() {
  const store = createStore<HeadroomState>(INITIAL_HEADROOM);
  return {
    ...store,
    settingsLoaded: (settings: HeadroomSettings) => store.update((s) => ({ ...s, settings })),
  };
}

export const headroomStore = createHeadroomStore();
