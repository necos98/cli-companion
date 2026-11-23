import { configStore } from "../stores/config";
import { headroomStore } from "../stores/headroom";
import { hubStore } from "../stores/hub";
import { layoutStore, viewWorkspaceId } from "../stores/layout";
import { uiStore } from "../stores/ui";
import type { World } from "../stores/ui";
import {
  activeWorld,
  managerShown,
  officeShown,
  overlayShown,
  sidebarFooterShown,
  worldRailShown,
} from "../stores/view";
import { useStore, useStores } from "./useStore";

// Cosa è mostrato, derivato dagli archivi (le funzioni pure stanno in
// `stores/view.ts`, usate anche dalle azioni). Gli archivi sono costanti di
// modulo: `useStores` li vuole stabili.

const LAYOUT_UI = [layoutStore, uiStore] as const;
const UI_CONFIG = [uiStore, configStore] as const;
const UI_HUB = [uiStore, hubStore] as const;
const VIEW_STORES = [layoutStore, uiStore, configStore, hubStore] as const;

const computeManagerShown = () => managerShown(layoutStore.getState(), uiStore.getState());
const computeOfficeShown = () => officeShown(uiStore.getState(), configStore.getState().botSettings);
const computeActiveWorld = () => activeWorld(uiStore.getState(), hubStore.getState());
const computeOverlayShown = () =>
  overlayShown(layoutStore.getState(), uiStore.getState(), configStore.getState().botSettings, hubStore.getState());

/** Workspace mostrato (quello a fuoco senza gruppo, altrimenti il gruppo attivo; null con la vista globale). */
export function useViewWorkspaceId(): string | null {
  return useStore(layoutStore, viewWorkspaceId);
}

/** Il Tasks Manager del workspace mostrato copre pannello Task e terminali. */
export function useManagerShown(): boolean {
  return useStores(LAYOUT_UI, computeManagerShown);
}

/** L'Ufficio è aperto e i bot sono attivi. */
export function useOfficeShown(): boolean {
  return useStores(UI_CONFIG, computeOfficeShown);
}

/** Il mondo mostrato: i terminali, o l'Agents HUB se acceso e scelto. */
export function useActiveWorld(): World {
  return useStores(UI_HUB, computeActiveWorld);
}

/** La barra dei mondi è visibile (solo con l'Agents HUB acceso). */
export function useWorldRailShown(): boolean {
  return useStore(hubStore, worldRailShown);
}

/** Impostazioni, Ufficio e versione stanno nel piede della sidebar (senza la barra dei mondi). */
export function useSidebarFooterShown(): boolean {
  return useStore(hubStore, sidebarFooterShown);
}

/** Manager, Ufficio o Agents HUB coprono pannello Task e terminali. */
export function useOverlayShown(): boolean {
  return useStores(VIEW_STORES, computeOverlayShown);
}

/** Headroom è acceso: la sua voce compare nella barra dei mondi. */
export function useHeadroomEnabled(): boolean {
  return useStore(headroomStore, (s) => s.settings.enabled);
}
