import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getVersion } from "@tauri-apps/api/app";
import { Loader2, SlidersHorizontal } from "lucide-react";
import Sidebar from "./components/Sidebar";
import type { SidebarTerminal } from "./components/Sidebar";
import AddWorkspaceDialog from "./components/AddWorkspaceDialog";
import SettingsDialog from "./components/SettingsDialog";
import type { SettingsTab } from "./components/SettingsDialog";
import TitleBar from "./components/TitleBar";
import type { TitleTab } from "./components/TitleBar";
import StatusBar from "./components/StatusBar";
import Logo from "./components/Logo";
import TerminalView from "./components/TerminalView";
import { Button } from "./components/ui/button";
import {
  api,
  onAgentEvent,
  onAutomationClose,
  onAutomationSpawn,
  onAutomationUpdate,
  onBoardUpdate,
  onNotificationActivated,
} from "./lib/tauri";
import { activeCount, mergeBoard } from "./lib/board";
import type { BoardView as BoardData } from "./lib/board";
import BoardView from "./components/BoardView";
import type {
  AgentEnvStatus,
  AgentProfile,
  BuiltinLaunch,
  Launch,
  Workspace,
} from "./lib/tauri";
import { allLaunches, launchCommands, resolveDefaultLaunch } from "./lib/launch";
import { WORKFLOW_WIZARD_COMMAND, WORKFLOW_WIZARD_TITLE, isAutoWorkspace } from "./lib/workflow";
import { automationBadgeText } from "./lib/schedule";
import {
  agentEnvBadge,
  applyAgentEvent,
  GROUP_STATUS,
  applyUserInput,
  countNeedingInput,
  installedAgentsSummary,
  notificationBody,
  resolveInputOverride,
  shouldSystemNotify,
  soundForEvent,
  tabAgentId,
  tabStatus,
} from "./lib/agents";
import type { AgentTabState, TabStatus } from "./lib/agents";
import { limitGroups, usageText, usageTooltip } from "./lib/usage";
import { useUsage } from "./lib/useUsage";
import {
  applyAutomationUpdate,
  automationTooltip,
  isAutomated,
  normalizeRole,
  ORCHESTRATOR_LABEL,
  ORCHESTRATOR_LAUNCH_ID,
  persistableTabs,
} from "./lib/automation";
import type { AutomationMeta } from "./lib/automation";
import {
  EMPTY_LAYOUT,
  GLOBAL_ID,
  activePaneId,
  closeTabIn,
  computePaneLayouts,
  focusTab,
  groupIndicator,
  openTab,
  openTabBackground,
  reorderTabs,
  selectGlobal,
  selectGroup,
  setActivePane,
  toggleMaximize,
  togglePin,
} from "./lib/layout";
import type { LayoutState } from "./lib/layout";
import {
  CLOSE_FLUSH_TIMEOUT_MS,
  CLAUDE_AGENT_ID,
  SESSION_SAVE_MAX_MS,
  SESSION_SAVE_MS,
  debounceDelay,
  parseSession,
  restoreCommand,
  serializeSession,
} from "./lib/session";
import type { SessionTab } from "./lib/session";
import { playAttention, playDing } from "./lib/notifySound";
import { notifyIfUnfocused } from "./lib/systemNotify";

interface Tab {
  key: string;
  workspace: Workspace;
  /** Avvio da cui è stato aperto il tab (assente = shell semplice). */
  launchId?: string;
  /** Comando dell'avvio ("" = nessuno): è quello che si salva nella sessione. */
  command: string;
  /**
   * Comando iniettato all'apertura del terminale: uguale a `command`, tranne
   * nei tab ripristinati (ripresa della conversazione dell'agente).
   */
  startCommand: string;
  /** Tab ripristinato da una sessione precedente (mostra la cronologia salvata). */
  restored?: boolean;
  title?: string;
  /**
   * Stato dell'agente (assente = tab normale con euristica busy/idle).
   * `agent.agentSessionId` serve a riprendere la sessione (`--resume`).
   */
  agent?: AgentTabState;
  /**
   * Sessione automatica (worker di un orchestratore): non si salva nella
   * sessione ne' si ripristina. Assente = tab normale.
   */
  automation?: AutomationMeta;
}

/** Etichetta mostrata per un tab (titolo OSC > nome avvio > nome workspace). */
function tabLabel(t: Tab, builtins: BuiltinLaunch[]): string {
  if (t.automation?.role === "orchestrator") return t.automation.title || ORCHESTRATOR_LABEL;
  // Un worker mostra il titolo del task: il titolo OSC dell'agente cambia di continuo.
  if (t.automation?.title) return t.automation.title;
  const launchName = t.launchId
    ? allLaunches(t.workspace, builtins).find((l) => l.id === t.launchId)?.name
    : undefined;
  return t.title ?? launchName ?? t.workspace.name;
}

function toSessionTab(t: Tab): SessionTab {
  return {
    tabId: t.key,
    workspaceId: t.workspace.id,
    launchId: t.launchId,
    command: t.command,
    title: t.title,
    agentId: t.agent?.agentId,
    agentSessionId: t.agent?.agentSessionId,
  };
}

function newTabKey(): string {
  try {
    return crypto.randomUUID();
  } catch {
    return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  }
}

export default function App() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("general-launches");
  const [settingsWorkspaceId, setSettingsWorkspaceId] = useState<string | undefined>();
  // `tabs` resta nell'ordine di creazione ed è l'ordine di render dei
  // TerminalView (mai riordinato: smontarli ucciderebbe la sessione pty).
  // L'ordine nella griglia vive solo in `layout`.
  const [tabs, setTabs] = useState<Tab[]>([]);
  // Avviso temporaneo (es. orchestratore gia' attivo), mostrato nella sidebar.
  const [notice, setNotice] = useState<string | null>(null);
  const noticeTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [layout, setLayout] = useState<LayoutState>(EMPTY_LAYOUT);
  const [busyByKey, setBusyByKey] = useState<Record<string, boolean>>({});
  const [drag, setDrag] = useState<{ from: string | null; over: string | null }>({
    from: null,
    over: null,
  });
  // Profili di input degli agenti (statici): letti una volta all'avvio.
  const [builtinLaunches, setBuiltinLaunches] = useState<BuiltinLaunch[]>([]);
  const builtinsRef = useRef(builtinLaunches);
  builtinsRef.current = builtinLaunches;
  const [globalLaunch, setGlobalLaunch] = useState<string | null>(null);
  const [agentProfiles, setAgentProfiles] = useState<AgentProfile[]>([]);
  const [sessionReady, setSessionReady] = useState(false);
  // Salvataggio abilitato solo se la lettura della sessione e' riuscita (file
  // assente compreso): dopo un errore di I/O non si sovrascrive session.json.
  const [saveEnabled, setSaveEnabled] = useState(false);
  // Plugin degli agenti installati per ambiente (barra di stato): caricati una
  // volta, in background, dopo l'avvio; aggiornati dalle Impostazioni.
  const [agentEnvs, setAgentEnvs] = useState<AgentEnvStatus[]>([]);
  // Utilizzo (limiti e token): interruttore globale delle Impostazioni.
  const [showUsage, setShowUsage] = useState(true);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  // Board dei task per workspace (riepilogo per la sidebar e vista a grafo).
  const [boards, setBoards] = useState<Record<string, BoardData>>({});
  const [boardErrors, setBoardErrors] = useState<Record<string, string>>({});
  // Modalità per workspace: true = si mostra la board al posto dei terminali.
  const [boardOpen, setBoardOpen] = useState<Record<string, boolean>>({});
  // Workspace mostrato senza avere un gruppo di terminali (il layout non lo
  // conosce): azzerato appena cambia la selezione del layout.
  const [boardFocus, setBoardFocus] = useState<string | null>(null);

  useEffect(() => {
    api
      .agentProfiles()
      .then(setAgentProfiles)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    api
      .listWorkspaces()
      .then((ws) => {
        setWorkspaces(ws);
        setLoading(false);
      })
      .catch((e) => {
        setLoadError(String(e));
        setLoading(false);
      });
  }, []);

  // Copia sempre aggiornata dei tab per il listener degli eventi (registrato
  // una volta sola).
  const tabsRef = useRef<Tab[]>([]);
  tabsRef.current = tabs;
  const workspacesRef = useRef<Workspace[]>([]);
  workspacesRef.current = workspaces;

  const paneLayouts = useMemo(() => computePaneLayouts(layout), [layout]);
  const paneLayoutsRef = useRef(paneLayouts);
  paneLayoutsRef.current = paneLayouts;

  const storeBoard = useCallback((b: BoardData) => {
    setBoards((prev) => ({ ...prev, [b.workspace_id]: mergeBoard(prev[b.workspace_id], b) }));
  }, []);

  // Caricamento (o ricaricamento, `force`) di una board. Con il backend assente
  // o in errore il riepilogo resta vuoto; l'errore si mostra solo nella vista.
  const loadBoard = useCallback(
    (workspaceId: string) => {
      api
        .boardGet(workspaceId)
        .then((b) => {
          storeBoard(b);
          setBoardErrors((prev) => {
            if (!(workspaceId in prev)) return prev;
            const next = { ...prev };
            delete next[workspaceId];
            return next;
          });
        })
        .catch((e) => setBoardErrors((prev) => ({ ...prev, [workspaceId]: String(e) })));
    },
    [storeBoard],
  );

  // Riepilogo delle board all'avvio e per i workspace aggiunti dopo.
  const boardRequestedRef = useRef(new Set<string>());
  useEffect(() => {
    for (const w of workspaces) {
      if (boardRequestedRef.current.has(w.id)) continue;
      boardRequestedRef.current.add(w.id);
      loadBoard(w.id);
    }
  }, [workspaces, loadBoard]);

  // Aggiornamenti della board dal backend.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    onBoardUpdate((b) => storeBoard(b))
      .then((u) => {
        if (cancelled) u();
        else unlisten = u;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [storeBoard]);

  // Ripristino della sessione precedente, una volta sola dopo il caricamento
  // dei workspace. Stessi tabId di prima: così combaciano lo scrollback
  // salvato e CLI_COMPANION_SESSION.
  const restoredRef = useRef(false);
  useEffect(() => {
    if (loading || loadError || restoredRef.current) return;
    restoredRef.current = true;
    void (async () => {
      let raw: string | null = null;
      let loaded = true;
      try {
        raw = await api.loadSession();
      } catch (e) {
        console.error("Lettura della sessione fallita, salvataggio disattivato:", e);
        loaded = false;
      }
      const data = parseSession(
        raw,
        workspaces.map((w) => w.id),
      );
      if (data) {
        const byId = new Map(workspaces.map((w) => [w.id, w]));
        setTabs(
          data.tabs.map(
            (t): Tab => ({
              key: t.tabId,
              workspace: byId.get(t.workspaceId)!,
              launchId: t.launchId,
              command: t.command,
              startCommand: restoreCommand(t),
              restored: true,
              title: t.title,
              agent: t.agentSessionId
                ? { phase: "idle", agentSessionId: t.agentSessionId, agentId: t.agentId }
                : undefined,
            }),
          ),
        );
        setLayout({
          groups: data.groups,
          activeWorkspaceId: data.activeWorkspaceId,
          global: data.global,
          activeView: data.activeView,
        });
      }
      // Orchestratori manuali ripristinati: registrati ignorando l'errore
      // (un duplicato di una vecchia sessione semplicemente non e' tracciato).
      for (const t of data?.tabs ?? []) {
        if (t.launchId === ORCHESTRATOR_LAUNCH_ID) {
          void api.automationOrchestratorOpen(t.workspaceId, t.tabId).catch(() => undefined);
        }
      }
      // Via lo scrollback dei tab che non esistono più (solo se la lettura è riuscita).
      if (loaded) {
        void api
          .pruneScrollback(data ? data.tabs.map((t) => t.tabId) : [])
          .catch(() => undefined);
      }
      setSaveEnabled(loaded);
      setSessionReady(true);
    })();
  }, [loading, loadError, workspaces]);

  // Salvataggio della sessione (debounce) a ogni modifica rilevante.
  const sessionJson = useMemo(
    () =>
      serializeSession({
        activeWorkspaceId: layout.activeWorkspaceId,
        activeView: layout.activeView,
        groups: layout.groups,
        global: layout.global,
        tabs: persistableTabs(tabs).map(toSessionTab),
      }),
    [tabs, layout],
  );
  const sessionJsonRef = useRef(sessionJson);
  sessionJsonRef.current = sessionJson;
  const savedJsonRef = useRef<string | null>(null);
  // Debounce con attesa massima: le modifiche frequenti (titoli OSC dello
  // spinner) non rimandano il salvataggio oltre SESSION_SAVE_MAX_MS.
  const pendingSinceRef = useRef<number | null>(null);
  useEffect(() => {
    if (!saveEnabled || sessionJson === savedJsonRef.current) {
      pendingSinceRef.current = null;
      return;
    }
    const now = Date.now();
    if (pendingSinceRef.current === null) pendingSinceRef.current = now;
    const delay = debounceDelay(
      now,
      pendingSinceRef.current,
      SESSION_SAVE_MS,
      SESSION_SAVE_MAX_MS,
    );
    const id = setTimeout(() => {
      pendingSinceRef.current = null;
      savedJsonRef.current = sessionJson;
      void api.saveSession(sessionJson).catch(() => {
        savedJsonRef.current = null;
      });
    }, delay);
    return () => clearTimeout(id);
  }, [sessionJson, saveEnabled]);

  // Salvataggi dello scrollback registrati dai TerminalView.
  const flushersRef = useRef(new Map<string, () => Promise<void>>());
  const registerFlush = useCallback(
    (tabId: string, flush: (() => Promise<void>) | null) => {
      if (flush) flushersRef.current.set(tabId, flush);
      else flushersRef.current.delete(tabId);
    },
    [],
  );

  // Alla chiusura della finestra: salva sessione e scrollback, con un tetto di
  // tempo così la chiusura non resta mai appesa. Senza preventDefault Tauri
  // chiude la finestra appena l'handler termina.
  const closingRef = useRef(false);
  const saveEnabledRef = useRef(false);
  saveEnabledRef.current = saveEnabled;
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    try {
      void getCurrentWindow()
        .onCloseRequested(async () => {
          closingRef.current = true;
          const work = Promise.all([
            saveEnabledRef.current
              ? api.saveSession(sessionJsonRef.current)
              : Promise.resolve(),
            ...[...flushersRef.current.values()].map((f) => f()),
          ]).catch(() => undefined);
          await Promise.race([
            work,
            new Promise((resolve) => setTimeout(resolve, CLOSE_FLUSH_TIMEOUT_MS)),
          ]);
        })
        .then((u) => {
          if (cancelled) u();
          else unlisten = u;
        })
        .catch(() => undefined);
    } catch {
      // fuori da Tauri (anteprima nel browser): nessun hook di chiusura
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Workspace mostrato e modalità: la board copre l'area dei pannelli di quel
  // workspace (mai nella vista Globale).
  const viewWorkspaceId =
    boardFocus ?? (layout.activeView === "global" ? null : layout.activeWorkspaceId);
  const boardShown = viewWorkspaceId !== null && boardOpen[viewWorkspaceId] === true;
  const modalOpenRef = useRef(false);
  modalOpenRef.current = settingsOpen || dialogOpen;
  const boardOpenRef = useRef(boardOpen);
  boardOpenRef.current = boardOpen;
  const viewWorkspaceIdRef = useRef<string | null>(null);
  viewWorkspaceIdRef.current = viewWorkspaceId;
  const layoutRef = useRef(layout);
  layoutRef.current = layout;

  // Il layout cambia selezione (gruppo, vista globale): il focus "senza gruppo" decade.
  useEffect(() => {
    setBoardFocus(null);
  }, [layout.activeWorkspaceId, layout.activeView]);

  // Un terminale messo a fuoco riporta ai terminali del suo workspace: la
  // board di quel workspace si chiude e il focus "senza gruppo" decade.
  const showTerminalsOf = useCallback((owner: string | undefined) => {
    if (owner) setBoardOpen((prev) => (prev[owner] ? { ...prev, [owner]: false } : prev));
    setBoardFocus(null);
  }, []);

  // Mostra/nasconde la board di un workspace. Se il workspace non è quello
  // mostrato lo seleziona e apre la board. Solo stato: i terminali restano montati.
  const toggleBoard = useCallback(
    (id: string) => {
      const current = viewWorkspaceIdRef.current;
      if (current === id) {
        const open = boardOpenRef.current[id] !== true;
        if (open) loadBoard(id);
        setBoardOpen((prev) => ({ ...prev, [id]: open }));
        return;
      }
      if (layoutRef.current.groups.some((g) => g.workspaceId === id)) {
        setBoardFocus(null);
        setLayout((l) => selectGroup(l, id));
      } else {
        setBoardFocus(id);
      }
      loadBoard(id);
      setBoardOpen((prev) => ({ ...prev, [id]: true }));
    },
    [loadBoard],
  );
  const toggleBoardRef = useRef(toggleBoard);
  toggleBoardRef.current = toggleBoard;

  // La board copre i terminali: nessuno deve tenere il focus della tastiera.
  useEffect(() => {
    if (!boardShown) return;
    const el = document.activeElement;
    if (el instanceof HTMLElement && el.closest(".xterm")) el.blur();
  }, [boardShown]);

  // "Apri terminale" dai dettagli di un task: torna ai terminali e mette a
  // fuoco il tab del worker (la chiave del tab è l'id di sessione).
  const handleOpenTerminal = useCallback((sessionId: string) => {
    const ws = viewWorkspaceIdRef.current;
    if (ws) setBoardOpen((prev) => ({ ...prev, [ws]: false }));
    setLayout((l) => focusTab(l, sessionId));
  }, []);

  // Ctrl+Maiusc+B: mostra/nasconde la board del workspace attivo (non nella
  // vista Globale). In fase di cattura, come Ctrl+Maiusc+M.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || !e.shiftKey || e.altKey || e.metaKey) return;
      if (e.key.toLowerCase() !== "b") return;
      // Con un dialogo aperto la scorciatoia non fa nulla.
      if (modalOpenRef.current) return;
      e.preventDefault();
      e.stopPropagation();
      const id = viewWorkspaceIdRef.current;
      if (id) toggleBoardRef.current(id);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  // Ctrl+Maiusc+M: massimizza/ripristina il pannello attivo della vista mostrata.
  // In fase di cattura, così xterm non vede il tasto. Niente Esc: serve alle CLI.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || !e.shiftKey || e.altKey || e.metaKey) return;
      if (e.key.toLowerCase() !== "m") return;
      e.preventDefault();
      e.stopPropagation();
      setLayout((l) => {
        const id = activePaneId(l);
        return id ? toggleMaximize(l, id) : l;
      });
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  // Polling dell'utilizzo: limiti per ambiente e token per sessione.
  const usage = useUsage(showUsage, agentEnvs, tabs);
  const usageEventRef = useRef(usage.onAgentEvent);
  usageEventRef.current = usage.onAgentEvent;

  // Eventi dei coding agent: stato del tab, suono e notifica di sistema.
  // `test` e' il self-test delle Impostazioni: solo il suono. Gli eventi il
  // cui tab non esiste (piu') sono ignorati.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    onAgentEvent((ev) => {
      if (ev.event === "test") {
        playDing();
        return;
      }
      const tab = tabsRef.current.find((t) => t.key === ev.tab_id);
      if (!tab) return;
      setTabs((prev) =>
        prev.map((t) =>
          t.key === tab.key ? { ...t, agent: applyAgentEvent(t.agent, ev) } : t,
        ),
      );
      usageEventRef.current(tab.key, ev.event, ev.transcript_path, ev.agent);
      const sound = soundForEvent(ev.event);
      if (sound === "attention") playAttention();
      else if (sound === "ding") playDing();
      if (shouldSystemNotify(ev.event)) {
        void notifyIfUnfocused(
          tab.key,
          tab.workspace.name,
          notificationBody(ev.event, tabLabel(tab, builtinsRef.current), ev.message),
        );
      }
    })
      .then((u) => {
        // Cleanup gia' eseguito (StrictMode): niente listener orfano.
        if (cancelled) u();
        else unlisten = u;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Click su una notifica di sistema: la finestra e' gia' in primo piano
  // (backend); qui si seleziona workspace e pannello. Solo stato del layout:
  // i TerminalView non si smontano ne' cambiano genitore. Tab chiuso: niente.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    onNotificationActivated((tabId) => {
      const owner = tabsRef.current.find((t) => t.key === tabId)?.workspace.id;
      showTerminalsOf(owner);
      setLayout((l) => focusTab(l, tabId));
    })
      .then((u) => {
        if (cancelled) u();
        else unlisten = u;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [showTerminalsOf]);

  // Senza `launch` parte l'avvio predefinito del workspace, se esiste;
  // altrimenti una shell semplice. Il nuovo terminale attiva il suo gruppo.
  // L'avvio "Orchestratore" va prima registrato nel backend (uno per workspace):
  // se rifiutato si mostra l'avviso e non si apre niente.
  const spawnTab = useCallback(async (ws: Workspace, launch?: Launch | BuiltinLaunch) => {
    const chosen = launch ?? resolveDefaultLaunch(ws, builtinLaunches, globalLaunch);
    const key = newTabKey();
    if (chosen?.id === ORCHESTRATOR_LAUNCH_ID) {
      try {
        await api.automationOrchestratorOpen(ws.id, key);
      } catch {
        setNotice("Un orchestratore è già attivo in questo workspace");
        clearTimeout(noticeTimerRef.current);
        noticeTimerRef.current = setTimeout(() => setNotice(null), 6000);
        return;
      }
    }
    // Per gli integrati si salva il comando di ripristino, si esegue quello di avvio.
    const { command, startCommand } = launchCommands(chosen);
    showTerminalsOf(ws.id);
    setTabs((prev) => [
      ...prev,
      { key, workspace: ws, launchId: chosen?.id, command, startCommand },
    ]);
    setLayout((l) => openTab(l, ws.id, key));
  }, [showTerminalsOf, builtinLaunches, globalLaunch]);

  // Wizard del workflow: chiude le impostazioni e apre in primo piano un
  // terminale del workspace che esegue Claude Code con la skill del plugin.
  const startWorkflowWizard = useCallback((ws: Workspace) => {
    const key = newTabKey();
    setSettingsOpen(false);
    showTerminalsOf(ws.id);
    setTabs((prev) => [
      ...prev,
      {
        key,
        workspace: ws,
        // Al ripristino il tab riprende la conversazione (`claude --resume <id>`)
        // invece di rilanciare l'intervista: si salva solo `claude`.
        command: "claude",
        startCommand: WORKFLOW_WIZARD_COMMAND,
        title: WORKFLOW_WIZARD_TITLE,
      },
    ]);
    setLayout((l) => openTab(l, ws.id, key));
  }, [showTerminalsOf]);

  // Controllo silenzioso all'avvio: se c'e' un update compare il pallino su
  // "Impostazioni". Errori e repo non configurato non fanno rumore.
  useEffect(() => {
    void (async () => {
      try {
        const cfg = await api.getUpdateConfig();
        if (!cfg.configured) return;
        const st = await api.checkUpdate();
        setUpdateAvailable(st.update_available);
      } catch {
        /* silenzioso */
      }
    })();
  }, []);

  // Avvii integrati e avvio predefinito globale: una volta, errori silenziosi.
  useEffect(() => {
    api.builtinLaunches().then(setBuiltinLaunches).catch(() => undefined);
    api
      .getLaunchSettings()
      .then((s) => {
        setGlobalLaunch(s.default_launch);
        setShowUsage(s.show_usage !== false);
      })
      .catch(() => undefined);
  }, []);

  // Versione dell'app (footer della sidebar).
  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch(() => undefined);
  }, []);

  // `agent_env_status` lancia processi (wsl.exe, claude): una sola volta, in
  // background e con un po' di ritardo, quando la sessione e' gia' ripristinata.
  // Non blocca la UI; dopo install/rimozione lo aggiornano le Impostazioni.
  useEffect(() => {
    if (!sessionReady) return;
    let cancelled = false;
    const id = setTimeout(() => {
      api
        .agentEnvStatus()
        .then((envs) => {
          if (!cancelled) setAgentEnvs(envs);
        })
        .catch(() => undefined);
    }, 1500);
    return () => {
      cancelled = true;
      clearTimeout(id);
    };
  }, [sessionReady]);

  const openLaunchSettings =useCallback((ws: Workspace) => {
    setSettingsTab("launches");
    setSettingsWorkspaceId(ws.id);
    setSettingsOpen(true);
  }, []);

  // Chiusura di un terminale (dall'utente o perché la shell è uscita): la sua
  // cronologia salvata non serve più.
  // Un tab automatico chiuso va comunicato al backend una volta sola, da
  // qualunque via arrivi la chiusura (utente, `automation-close`, shell uscita).
  const reportedClosedRef = useRef(new Set<string>());
  const closeTab = useCallback((key: string) => {
    const closing = tabsRef.current.find((t) => t.key === key);
    const automated = closing !== undefined && isAutomated(closing);
    // Anche l'orchestratore manuale va comunicato: libera il posto nel workspace.
    const reportable = automated || closing?.launchId === ORCHESTRATOR_LAUNCH_ID;
    if (reportable && !reportedClosedRef.current.has(key)) {
      reportedClosedRef.current.add(key);
      void api.automationTabClosed(key).catch(() => undefined);
    }
    setTabs((prev) => prev.filter((t) => t.key !== key));
    setLayout((l) => closeTabIn(l, key));
    setBusyByKey((prev) => {
      if (!(key in prev)) return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
    // Gli automatici non salvano lo scrollback: niente da cancellare.
    if (!automated) void api.deleteScrollback(key).catch(() => undefined);
  }, []);

  // La shell è uscita da sola. Durante la chiusura dell'app i pty vengono
  // chiusi: non è un'uscita dell'utente, la sessione va conservata.
  const handleExit = useCallback(
    (key: string) => {
      if (closingRef.current) return;
      closeTab(key);
    },
    [closeTab],
  );

  const closeTabRef = useRef(closeTab);
  closeTabRef.current = closeTab;

  // Sessioni automatiche: il backend chiede di aprire/chiudere i tab dei worker
  // e ne comunica gli stati. Registrato una volta sola (stato letto dai ref).
  useEffect(() => {
    const unlistens: Array<() => void> = [];
    let cancelled = false;
    const keep = (p: Promise<() => void>) =>
      p
        .then((u) => {
          if (cancelled) u();
          else unlistens.push(u);
        })
        .catch(() => undefined);

    keep(
      onAutomationSpawn((ev) => {
        // Workspace sconosciuto (rimosso nel frattempo) o tab gia' aperto: ignora.
        const ws = workspacesRef.current.find((w) => w.id === ev.workspace_id);
        if (!ws || tabsRef.current.some((t) => t.key === ev.tab_id)) return;
        const tab: Tab = {
          key: ev.tab_id,
          workspace: ws,
          command: "",
          startCommand: ev.command,
          automation: {
            role: normalizeRole(ev.role),
            parentTabId: ev.parent_tab_id ?? "",
            title: ev.title,
            status: "starting",
            agentId: ev.agent_id,
            cwd: ev.cwd ?? undefined,
          },
        };
        // Aggiornamento sincrono del ref: un secondo evento identico non riapre il tab.
        tabsRef.current = [...tabsRef.current, tab];
        setTabs((prev) => (prev.some((t) => t.key === tab.key) ? prev : [...prev, tab]));
        // In secondo piano: non toglie all'utente la vista in cui sta lavorando.
        setLayout((l) => openTabBackground(l, ws.id, tab.key));
      }),
    );

    keep(onAutomationClose((ev) => closeTabRef.current(ev.tab_id)));

    keep(
      onAutomationUpdate((ev) =>
        setTabs((prev) =>
          prev.map((t) =>
            t.key === ev.tab_id && t.automation
              ? { ...t, automation: applyAutomationUpdate(t.automation, ev) }
              : t,
          ),
        ),
      ),
    );

    // Riallinea i metadati dei tab gia' aperti (es. dopo un ricaricamento).
    api
      .automationSessions()
      .then((infos) => {
        if (cancelled) return;
        const byKey = new Map(infos.map((i) => [i.tab_id, i]));
        setTabs((prev) =>
          prev.map((t) => {
            const info = byKey.get(t.key);
            return info && t.automation
              ? { ...t, automation: applyAutomationUpdate(t.automation, info) }
              : t;
          }),
        );
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
      unlistens.forEach((u) => u());
    };
  }, []);

  const handleTitle = useCallback((key: string, title: string) => {
    setTabs((prev) => prev.map((t) => (t.key === key ? { ...t, title } : t)));
  }, []);

  // Input dell'utente: Invio => l'agente lavora, altro input toglie ✓/❗.
  const handleInput = useCallback((key: string, enter: boolean) => {
    setTabs((prev) => {
      const i = prev.findIndex((t) => t.key === key);
      if (i < 0) return prev;
      const next = applyUserInput(prev[i].agent, enter);
      if (next === prev[i].agent) return prev;
      const copy = prev.slice();
      copy[i] = { ...prev[i], agent: next };
      return copy;
    });
  }, []);

  const handleStatus = useCallback((key: string, busy: boolean) => {
    setBusyByKey((prev) => (prev[key] === busy ? prev : { ...prev, [key]: busy }));
  }, []);

  // Trascinamento delle intestazioni: l'origine sta in un ref (letta anche
  // negli handler), lo stato `drag` serve solo all'evidenziazione.
  const dragFromRef = useRef<string | null>(null);

  const handlePaneDragStart = useCallback((id: string) => {
    dragFromRef.current = id;
    setDrag({ from: id, over: null });
  }, []);

  const handlePaneDragEnd = useCallback(() => {
    dragFromRef.current = null;
    setDrag({ from: null, over: null });
  }, []);

  const handlePaneDragOver = useCallback((id: string) => {
    const from = dragFromRef.current;
    const pl = paneLayoutsRef.current;
    const ok =
      from !== null &&
      from !== id &&
      pl[from]?.groupWorkspaceId === pl[id]?.groupWorkspaceId;
    const over = ok ? id : null;
    setDrag((d) => (d.over === over ? d : { ...d, over }));
  }, []);

  const handlePaneDrop = useCallback(
    (id: string) => {
      const from = dragFromRef.current;
      if (from && from !== id) setLayout((l) => reorderTabs(l, from, id));
      handlePaneDragEnd();
    },
    [handlePaneDragEnd],
  );

  const handleAdded = useCallback((ws: Workspace) => {
    setWorkspaces((prev) => [...prev, ws]);
  }, []);

  // Aggiorna lo stato con il Workspace restituito dal backend (rinomina,
  // modifica degli avvii) anche nei tab già aperti.
  const handleWorkspaceUpdated = useCallback((updated: Workspace) => {
    setWorkspaces((prev) => prev.map((w) => (w.id === updated.id ? updated : w)));
    setTabs((prev) =>
      prev.map((t) =>
        t.workspace.id === updated.id ? { ...t, workspace: updated } : t,
      ),
    );
  }, []);

  const handleRename = useCallback(
    async (id: string, name: string) => {
      handleWorkspaceUpdated(await api.renameWorkspace(id, name));
    },
    [handleWorkspaceUpdated],
  );

  const handleRemove = useCallback(
    async (id: string) => {
      await api.removeWorkspace(id);
      // Stessa via della chiusura da parte dell'utente: pty (smontaggio del
      // TerminalView), scrollback, gruppo e selezione di un altro gruppo.
      for (const t of tabsRef.current) {
        if (t.workspace.id === id) closeTab(t.key);
      }
      setWorkspaces((prev) => prev.filter((w) => w.id !== id));
    },
    [closeTab],
  );

  const handleSelectWorkspace = useCallback((id: string) => {
    // Un workspace senza terminali non ha un gruppo: lo si "mostra" comunque
    // (con la sua modalità, utile per aprirne la board).
    const hasGroup = layoutRef.current.groups.some((g) => g.workspaceId === id);
    setBoardFocus(hasGroup ? null : id);
    setLayout((l) => selectGroup(l, id));
  }, []);

  const globalActive = layout.activeView === "global";
  const globalCount = layout.global.order.length;

  const statusOf = (t: Tab): TabStatus => tabStatus(t.agent, busyByKey[t.key] === true);
  const statuses = tabs.map(statusOf);
  const statusByKey = new Map(tabs.map((t, i) => [t.key, statuses[i]]));
  const indicatorStatus = (keys: string[]): TabStatus | null => {
    const ind = groupIndicator(
      keys.flatMap((k) => {
        const s = statusByKey.get(k);
        return s ? [s] : [];
      }),
    );
    return ind === "none" ? null : GROUP_STATUS[ind];
  };

  // Tab della barra del titolo: vista globale (se ha tab) + un tab per gruppo.
  const globalTab: TitleTab | null =
    globalCount > 0
      ? {
          id: GLOBAL_ID,
          name: "Globale",
          count: globalCount,
          status: indicatorStatus(layout.global.order),
        }
      : null;
  const groupTabs: TitleTab[] = layout.groups.map((g) => ({
    id: g.workspaceId,
    name:
      tabs.find((t) => t.workspace.id === g.workspaceId)?.workspace.name ?? g.workspaceId,
    count: g.order.length,
    status: indicatorStatus(g.order),
  }));

  // Terminali di ogni workspace per la sidebar (ordine della griglia).
  const tabByKey = new Map(tabs.map((t) => [t.key, t]));
  const sidebarTerminals: Record<string, SidebarTerminal[]> = {};
  for (const g of layout.groups) {
    sidebarTerminals[g.workspaceId] = g.order.flatMap((key) => {
      const t = tabByKey.get(key);
      if (!t) return [];
      return [
        {
          key,
          label: tabLabel(t, builtinLaunches),
          status: statusByKey.get(key)!,
          pinned: layout.global.order.includes(key),
          active: g.activeTabId === key,
        },
      ];
    });
  }

  const attentionCount = countNeedingInput(statuses);
  const agentsSummary = installedAgentsSummary(agentEnvs);
  const limitEntries = showUsage
    ? limitGroups(
        agentEnvs
          .filter((e) => e.agent_id === CLAUDE_AGENT_ID && e.found && e.installed)
          .map((e) => ({
            envKey: e.env_key,
            label: agentEnvBadge(e),
            reply: usage.limits[e.env_key],
          })),
      )
    : [];
  const anyPaneVisible = Object.values(paneLayouts).some((p) => p.visible);

  return (
    <div className="flex h-screen flex-col">
      <TitleBar
        globalTab={globalTab}
        globalActive={globalActive}
        groups={groupTabs}
        activeWorkspaceId={layout.activeWorkspaceId}
        onSelectGlobal={() => setLayout(selectGlobal)}
        onSelectGroup={handleSelectWorkspace}
      />

      <div className="flex min-h-0 flex-1">
        <aside className="flex w-[260px] flex-shrink-0 flex-col overflow-hidden border-r bg-base">
          {loadError ? (
            <div className="p-4">
              <p className="text-[13px] text-destructive">{loadError}</p>
            </div>
          ) : loading || !sessionReady ? (
            <div className="flex items-center gap-2 p-4 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              Caricamento…
            </div>
          ) : (
            <Sidebar
              workspaces={workspaces}
              activeWorkspaceId={viewWorkspaceId}
              terminals={sidebarTerminals}
              boardActive={Object.fromEntries(
                Object.entries(boards).map(([id, b]) => [id, activeCount(b)]),
              )}
              autoStatus={Object.fromEntries(
                workspaces.flatMap((w) =>
                  isAutoWorkspace(w)
                    ? [
                        [
                          w.id,
                          automationBadgeText(boards[w.id]?.automation) ??
                            "Orchestratore automatico attivo",
                        ],
                      ]
                    : [],
                ),
              )}
              boardShownId={boardShown ? viewWorkspaceId : null}
              onToggleBoard={toggleBoard}
              onSelect={handleSelectWorkspace}
              onFocusTerminal={(key) => {
                // Un terminale scelto dalla sidebar riporta ai terminali del suo workspace.
                showTerminalsOf(tabsRef.current.find((t) => t.key === key)?.workspace.id);
                setLayout((l) => focusTab(l, key));
              }}
              builtinLaunches={builtinLaunches}
              globalLaunch={globalLaunch}
              onSpawn={spawnTab}
              notice={notice}
              onManageLaunches={openLaunchSettings}
              onRename={handleRename}
              onRemove={handleRemove}
              onAdd={() => setDialogOpen(true)}
            />
          )}
          <div className="flex items-center gap-2 border-t px-3 py-2.5">
            <Button
              variant="ghost"
              onClick={() => {
                setSettingsTab(updateAvailable ? "updates" : "general-launches");
                setSettingsWorkspaceId(undefined);
                setSettingsOpen(true);
              }}
              className="h-auto flex-1 justify-start gap-2 px-2 py-[7px] text-[13px] font-normal text-text-secondary hover:text-foreground"
            >
              <SlidersHorizontal className="!size-[15px]" />
              Impostazioni
              {updateAvailable && (
                <span
                  className="h-1.5 w-1.5 rounded-full bg-primary"
                  title="Aggiornamento disponibile"
                  role="img"
                  aria-label="Aggiornamento disponibile"
                />
              )}
            </Button>
            {appVersion && (
              <span className="font-mono text-[11px] text-faint">v{appVersion}</span>
            )}
          </div>
        </aside>

        <div className="flex min-w-0 flex-1 flex-col">
          {/* Area dei pannelli. TUTTI i TerminalView di TUTTI i gruppi sono figli
              diretti di questo contenitore, con chiave stabile e nell'ordine di
              creazione: layout, riordino, cambio gruppo e maximize cambiano solo
              lo stile (rettangolo / display), mai la struttura del DOM, quindi
              nessun terminale viene smontato o spostato (ucciderebbe il pty).
              Il contenitore interno e' fisso (inset 3px + 3px per pannello =
              6px di margine e di spazio fra i pannelli). */}
          <div className="relative min-h-0 flex-1 bg-pane-area">
            <div
              className="absolute inset-[3px]"
              aria-hidden={boardShown || undefined}
              ref={(el) => {
                // `inert` solo come attributo (niente remount): i terminali
                // nascosti dalla board non restano raggiungibili con Tab.
                if (!el) return;
                if (boardShown) el.setAttribute("inert", "");
                else el.removeAttribute("inert");
              }}
            >
              {!anyPaneVisible && (
                <div className="absolute inset-[3px] flex flex-col items-center justify-center gap-3 rounded-[10px] border bg-pane-area p-5">
                  <Logo size={40} />
                  <span className="text-[13px] font-medium">Nessun terminale aperto</span>
                  <span className="text-center text-xs text-muted-foreground">
                    Apri un terminale da un workspace, o con un avvio dal menu ⋯
                  </span>
                </div>
              )}
              {tabs.map((t) => {
                const pl = paneLayouts[t.key];
                return (
                  <TerminalView
                    key={t.key}
                    tabId={t.key}
                    workspace={t.workspace}
                    initialCommand={t.startCommand}
                    imagePasteSeq={resolveInputOverride(
                      agentProfiles,
                      tabAgentId(t),
                      t.workspace.kind,
                      "image-paste",
                    )}
                    restore={t.restored}
                    rect={pl?.rect ?? null}
                    active={pl?.visible ?? false}
                    focused={(pl?.focused ?? false) && !boardShown}
                    label={tabLabel(t, builtinLaunches)}
                    labelPrefix={
                      pl?.groupWorkspaceId === GLOBAL_ID ? t.workspace.name : undefined
                    }
                    status={statusOf(t)}
                    cwd={t.automation?.cwd}
                    launchId={t.launchId}
                    usage={
                      showUsage && usage.sessions[t.key] && !t.agent?.ended
                        ? {
                            text: usageText(usage.sessions[t.key]),
                            title: usageTooltip(usage.sessions[t.key]),
                          }
                        : undefined
                    }
                    automation={
                      t.automation
                        ? {
                            role: t.automation.role,
                            status: t.automation.status,
                            tooltip: automationTooltip(
                              t.automation,
                              tabByKey.get(t.automation.parentTabId)
                                ? tabLabel(tabByKey.get(t.automation.parentTabId)!, builtinLaunches)
                                : undefined,
                            ),
                          }
                        : undefined
                    }
                    maximized={pl?.maximized ?? false}
                    pinned={pl?.pinned ?? false}
                    dropTarget={drag.over === t.key}
                    onActivate={() => setLayout((l) => setActivePane(l, t.key))}
                    onClose={() => closeTab(t.key)}
                    onToggleMaximize={() => setLayout((l) => toggleMaximize(l, t.key))}
                    onTogglePin={() => setLayout((l) => togglePin(l, t.key))}
                    onPaneDragStart={() => handlePaneDragStart(t.key)}
                    onPaneDragOver={() => handlePaneDragOver(t.key)}
                    onPaneDragLeave={() =>
                      setDrag((d) => (d.over === t.key ? { ...d, over: null } : d))
                    }
                    onPaneDrop={() => handlePaneDrop(t.key)}
                    onPaneDragEnd={handlePaneDragEnd}
                    registerFlush={registerFlush}
                    onExit={() => handleExit(t.key)}
                    onTitle={(title) => handleTitle(t.key, title)}
                    onStatus={(busy) => handleStatus(t.key, busy)}
                    onInput={(enter) => handleInput(t.key, enter)}
                  />
                );
              })}
            </div>
            {/* La board copre l'area dei pannelli senza toccarli: i TerminalView
                qui sopra restano montati e al loro posto. */}
            {boardShown && viewWorkspaceId && (
              <div className="absolute inset-[3px] z-20 overflow-hidden rounded-[10px] border">
                <BoardView
                  key={viewWorkspaceId}
                  workspaceId={viewWorkspaceId}
                  board={boards[viewWorkspaceId] ?? null}
                  loadError={boardErrors[viewWorkspaceId] ?? null}
                  onBoard={storeBoard}
                  onOpenTerminal={handleOpenTerminal}
                />
              </div>
            )}
          </div>
        </div>
      </div>

      <StatusBar
        terminalCount={tabs.length}
        attentionCount={attentionCount}
        agentsSummary={agentsSummary}
        limits={limitEntries}
      />

      <AddWorkspaceDialog
        isOpen={dialogOpen}
        onClose={() => setDialogOpen(false)}
        onAdded={handleAdded}
      />
      <SettingsDialog
        isOpen={settingsOpen}
        initialTab={settingsTab}
        initialWorkspaceId={settingsWorkspaceId}
        workspaces={workspaces}
        onClose={() => setSettingsOpen(false)}
        onWorkspaceUpdated={handleWorkspaceUpdated}
        onUpdateAvailable={setUpdateAvailable}
        updateAvailable={updateAvailable}
        onAgentEnvs={setAgentEnvs}
        onShowUsage={setShowUsage}
        agentProfiles={agentProfiles}
        agentEnvs={agentEnvs}
        builtinLaunches={builtinLaunches}
        globalLaunch={globalLaunch}
        onGlobalLaunchChange={setGlobalLaunch}
        onCreateWorkflow={startWorkflowWizard}
      />
    </div>
  );
}
