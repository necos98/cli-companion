import { useEffect } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { loadHubSettings } from "../app/actions/hub";
import { loadHeadroomSettings } from "../app/actions/headroom";
import { api } from "../lib/tauri";
import { ignoreError, report } from "../lib/errors";
import { configStore } from "../stores/config";
import { workspacesStore } from "../stores/workspaces";

/**
 * Dati letti una volta all'avvio e scritti negli archivi: workspace, profili
 * degli agenti, impostazioni degli avvii e versione dell'app.
 */
export function useInitialData(): void {
  useEffect(() => {
    api
      .agentProfiles()
      .then((agentProfiles) => configStore.patch({ agentProfiles }))
      .catch((e) => report("caricamento dei profili degli agenti", e, { notify: true }))
      // Profili caricati (o errore già segnalato): il ripristino della sessione li aspetta.
      .finally(() => configStore.patch({ profilesReady: true }));
  }, []);

  useEffect(() => {
    api
      .listWorkspaces()
      .then((list) => workspacesStore.loaded(list))
      .catch((e) => workspacesStore.failed(String(e)));
  }, []);

  // Avvio predefinito globale: una volta (gli avvii integrati li legge `useLaunchesSync`).
  // Senza di lui l'app userebbe i valori di default: l'utente va avvisato.
  useEffect(() => {
    api
      .getLaunchSettings()
      .then((s) => configStore.patch({ globalLaunch: s.default_launch, showUsage: s.show_usage }))
      .catch((e) => report("caricamento delle impostazioni degli avvii", e, { notify: true }));
  }, []);

  // Interruttore dell'Agents HUB (spento di default).
  useEffect(() => {
    void loadHubSettings();
  }, []);

  // Headroom: interruttore e porta (la dashboard sta nella barra dei mondi).
  useEffect(() => {
    void loadHeadroomSettings();
  }, []);

  // Versione dell'app (footer della sidebar).
  useEffect(() => {
    getVersion()
      .then((appVersion) => configStore.patch({ appVersion }))
      .catch(ignoreError("la versione è solo un'etichetta nel footer; fuori da Tauri non esiste"));
  }, []);
}
