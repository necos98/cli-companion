import { useEffect, useRef } from "react";
import { api } from "../lib/tauri";
import { appWindow } from "../lib/appWindow";
import { report } from "../lib/errors";
import { startSubscription } from "../lib/subscription";
import { noticeStore } from "../lib/notices";
import { parseSession, unavailableAgentsNotice } from "../lib/session";
import { loadHubSettings } from "../app/actions/hub";
import { stageBotRestoreEnvs, stageBuiltinRestoreEnvs } from "../app/botRestore";
import { flushBeforeExit } from "../app/lifecycle";
import { sessionSaver, watchSessionChanges } from "../app/sessionSaver";
import { configStore } from "../stores/config";
import { hubStore, reservedBotNames } from "../stores/hub";
import { layoutStore } from "../stores/layout";
import { mergeRestoredLayout, restoredTabs } from "../stores/session";
import { runtimeStore } from "../stores/tabRuntime";
import { tabsStore } from "../stores/tabs";
import { uiStore } from "../stores/ui";
import { visibleWorkspaces, workspacesStore } from "../stores/workspaces";
import { useStore } from "./useStore";

/**
 * Ripristino della sessione precedente. Stessi tabId di prima: così
 * combaciano lo scrollback salvato e CLI_COMPANION_SESSION. I tab ripristinati
 * si UNISCONO a quelli già presenti (worker spawnati prima del ripristino).
 */
async function restoreSession(): Promise<void> {
  // Workspace al momento in cui parte il ripristino (gli archiviati non ripristinano nulla).
  const workspaceIds = visibleWorkspaces(workspacesStore.getState()).map((w) => w.id);
  let raw: string | null = null;
  let loaded = true;
  try {
    raw = await api.loadSession();
  } catch (e) {
    console.error("Lettura della sessione fallita, salvataggio disattivato:", e);
    loaded = false;
  }
  const data = parseSession(raw, workspaceIds);
  // Bot: nome e avatar di ogni sessione, caricati col ripristino (i nomi dei
  // tab ripristinati dipendono dalla pool).
  const bots = await api.getBotSettings().catch((e) => {
    report("caricamento delle impostazioni dei bot", e, { notify: true });
    return configStore.getState().botSettings;
  });
  configStore.patch({ botSettings: bots });
  // I nomi dei segretari sono riservati: le impostazioni dell'HUB servono prima di assegnare i nomi.
  await loadHubSettings();
  if (data) {
    const restored = restoredTabs(
      data,
      tabsStore.getState(),
      bots.names,
      configStore.getState().agentProfiles,
      undefined,
      reservedBotNames(hubStore.getState()),
    );
    for (const [key, init] of Object.entries(restored.runtime)) runtimeStore.seed(key, init);
    if (restored.unavailableAgents.length > 0) {
      noticeStore.push({
        key: "session:unavailable-agents",
        level: "warning",
        text: unavailableAgentsNotice(restored.unavailableAgents),
      });
    }
    // L'ambiente dei tab Bot va preparato prima che i terminali partano.
    const botWarning = await stageBotRestoreEnvs(restored.tabs);
    if (botWarning !== null) noticeStore.push({ key: "session:bot-restore", level: "warning", text: botWarning });
    const headroomWarning = await stageBuiltinRestoreEnvs(restored.tabs);
    if (headroomWarning !== null) {
      noticeStore.push({ key: "session:headroom", level: "warning", text: headroomWarning });
    }
    tabsStore.merge(restored.tabs);
    const restoredLayout = {
      groups: data.groups,
      activeWorkspaceId: data.activeWorkspaceId,
      global: data.global,
      activeView: data.activeView,
    };
    layoutStore.apply((current) => mergeRestoredLayout(restoredLayout, current));
  }
  // Via lo scrollback dei tab che non esistono più (solo se la lettura è riuscita).
  if (loaded) {
    void api
      .pruneScrollback(data ? data.tabs.map((t) => t.tabId) : [])
      .catch((e) => report("pulizia della cronologia salvata", e));
  }
  // Salvataggio abilitato solo se la lettura della sessione è riuscita (file
  // assente compreso): dopo un errore di I/O non si sovrascrive session.json.
  if (loaded) sessionSaver.enable();
  uiStore.setSessionReady(true);
}

/**
 * Persistenza della sessione: ripristino una volta sola dopo il caricamento
 * dei workspace e dei profili, salvataggio (debounce) a ogni modifica
 * rilevante e salvataggio finale alla chiusura della finestra.
 */
export function useSessionPersistence(): void {
  const workspacesStatus = useStore(workspacesStore, (s) => s.status);
  const profilesReady = useStore(configStore, (s) => s.profilesReady);
  const sessionReady = useStore(uiStore, (s) => s.sessionReady);

  const restoredRef = useRef(false);
  useEffect(() => {
    if (workspacesStatus !== "ready" || !profilesReady || restoredRef.current) return;
    restoredRef.current = true;
    void restoreSession();
  }, [workspacesStatus, profilesReady]);

  // Salvataggio a ogni modifica rilevante (i titoli da soli non contano).
  useEffect(() => {
    if (!sessionReady) return;
    return watchSessionChanges();
  }, [sessionReady]);

  // Alla chiusura della finestra: salva sessione e scrollback. Senza
  // preventDefault Tauri chiude la finestra appena l'handler termina.
  useEffect(() => {
    const win = appWindow();
    // Fuori da Tauri (anteprima nel browser) non c'è nessun hook di chiusura.
    if (!win) return;
    return startSubscription(
      () => win.onCloseRequested(flushBeforeExit),
      (e) => report("registrazione del salvataggio alla chiusura", e),
    );
  }, []);
}
