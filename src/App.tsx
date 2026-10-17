import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Loader2, Settings, Star } from "lucide-react";
import Sidebar from "./components/Sidebar";
import AddWorkspaceDialog from "./components/AddWorkspaceDialog";
import SettingsDialog from "./components/SettingsDialog";
import type { SettingsTab } from "./components/SettingsDialog";
import StatusIcon from "./components/StatusIcon";
import TerminalView from "./components/TerminalView";
import { Button } from "./components/ui/button";
import { api, onAgentEvent, onNotificationActivated } from "./lib/tauri";
import type { AgentProfile, Launch, Workspace } from "./lib/tauri";
import { defaultLaunch } from "./lib/launch";
import {
  applyAgentEvent,
  applyUserInput,
  notificationBody,
  resolveInputOverride,
  shouldSystemNotify,
  soundForEvent,
  tabAgentId,
  tabStatus,
} from "./lib/agents";
import type { AgentTabState, TabStatus } from "./lib/agents";
import {
  EMPTY_LAYOUT,
  GLOBAL_ID,
  activePaneId,
  closeTabIn,
  computePaneLayouts,
  focusTab,
  groupIndicator,
  openTab,
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
import { cn } from "./lib/utils";

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
}

/** Etichetta mostrata per un tab (titolo OSC > nome avvio > nome workspace). */
function tabLabel(t: Tab): string {
  const launchName = t.launchId
    ? t.workspace.launches.find((l) => l.id === t.launchId)?.name
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

const GROUP_STATUS: Record<"alert" | "check" | "busy", TabStatus> = {
  alert: { icon: "alert", tone: "attention", label: "Un terminale aspetta una risposta" },
  check: { icon: "check", tone: "done", label: "Un agente ha finito" },
  busy: { icon: "dot", tone: "busy", label: "Terminali al lavoro" },
};

export default function App() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("launches");
  const [settingsWorkspaceId, setSettingsWorkspaceId] = useState<string | undefined>();
  // `tabs` resta nell'ordine di creazione ed è l'ordine di render dei
  // TerminalView (mai riordinato: smontarli ucciderebbe la sessione pty).
  // L'ordine nella griglia vive solo in `layout`.
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [layout, setLayout] = useState<LayoutState>(EMPTY_LAYOUT);
  const [busyByKey, setBusyByKey] = useState<Record<string, boolean>>({});
  const [drag, setDrag] = useState<{ from: string | null; over: string | null }>({
    from: null,
    over: null,
  });
  // Profili di input degli agenti (statici): letti una volta all'avvio.
  const [agentProfiles, setAgentProfiles] = useState<AgentProfile[]>([]);
  const [sessionReady, setSessionReady] = useState(false);
  // Salvataggio abilitato solo se la lettura della sessione e' riuscita (file
  // assente compreso): dopo un errore di I/O non si sovrascrive session.json.
  const [saveEnabled, setSaveEnabled] = useState(false);

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

  const paneLayouts = useMemo(() => computePaneLayouts(layout), [layout]);
  const paneLayoutsRef = useRef(paneLayouts);
  paneLayoutsRef.current = paneLayouts;

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
        tabs: tabs.map(toSessionTab),
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
      const sound = soundForEvent(ev.event);
      if (sound === "attention") playAttention();
      else if (sound === "ding") playDing();
      if (shouldSystemNotify(ev.event)) {
        void notifyIfUnfocused(
          tab.key,
          tab.workspace.name,
          notificationBody(ev.event, tabLabel(tab), ev.message),
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
    onNotificationActivated((tabId) => setLayout((l) => focusTab(l, tabId)))
      .then((u) => {
        if (cancelled) u();
        else unlisten = u;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Senza `launch` parte l'avvio predefinito del workspace, se esiste;
  // altrimenti una shell semplice. Il nuovo terminale attiva il suo gruppo.
  const spawnTab = useCallback((ws: Workspace, launch?: Launch) => {
    const chosen = launch ?? defaultLaunch(ws);
    const key = newTabKey();
    const command = chosen?.command ?? "";
    setTabs((prev) => [
      ...prev,
      { key, workspace: ws, launchId: chosen?.id, command, startCommand: command },
    ]);
    setLayout((l) => openTab(l, ws.id, key));
  }, []);

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

  const openLaunchSettings = useCallback((ws: Workspace) => {
    setSettingsTab("launches");
    setSettingsWorkspaceId(ws.id);
    setSettingsOpen(true);
  }, []);

  // Chiusura di un terminale (dall'utente o perché la shell è uscita): la sua
  // cronologia salvata non serve più.
  const closeTab = useCallback((key: string) => {
    setTabs((prev) => prev.filter((t) => t.key !== key));
    setLayout((l) => closeTabIn(l, key));
    setBusyByKey((prev) => {
      if (!(key in prev)) return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
    void api.deleteScrollback(key).catch(() => undefined);
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
    setLayout((l) => selectGroup(l, id));
  }, []);

  const globalActive = layout.activeView === "global";
  const globalCount = layout.global.order.length;

  const statusOf = (t: Tab): TabStatus => tabStatus(t.agent, busyByKey[t.key] === true);
  const globalIndicator = groupIndicator(
    tabs.filter((t) => layout.global.order.includes(t.key)).map(statusOf),
  );

  return (
    <div className="flex h-screen">
      <aside className="flex w-[300px] min-w-[260px] max-w-[340px] flex-col overflow-hidden border-r bg-card">
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
            activeWorkspaceId={globalActive ? null : layout.activeWorkspaceId}
            onSelect={handleSelectWorkspace}
            onSpawn={spawnTab}
            onManageLaunches={openLaunchSettings}
            onRename={handleRename}
            onRemove={handleRemove}
            onAdd={() => setDialogOpen(true)}
          />
        )}
        <div className="flex flex-col gap-2 border-t p-3">
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              setSettingsTab(updateAvailable ? "updates" : "launches");
              setSettingsWorkspaceId(undefined);
              setSettingsOpen(true);
            }}
            className="w-full justify-start"
          >
            <Settings className="h-3.5 w-3.5" />
            Impostazioni
            {updateAvailable && (
              <span
                className="ml-auto h-2 w-2 rounded-full bg-primary"
                title="Aggiornamento disponibile"
                aria-label="Aggiornamento disponibile"
              />
            )}
          </Button>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        {/* Barra dei workspace: un tab per gruppo di terminali. */}
        <div className="flex flex-shrink-0 items-center gap-1.5 overflow-x-auto border-b bg-card px-2.5 py-2">
          {layout.groups.length === 0 && (
            <span className="px-2 text-xs text-muted-foreground">
              Nessun terminale aperto
            </span>
          )}
          {/* Vista globale: sempre prima, mostrata solo con almeno un tab fissato. */}
          {globalCount > 0 && (
            <div
              key={GLOBAL_ID}
              onClick={() => setLayout(selectGlobal)}
              title="Vista globale: terminali fissati di tutti i workspace"
              aria-label={`Vista globale: ${globalCount} terminali`}
              className={cn(
                "flex cursor-pointer select-none items-center gap-2 rounded-md border px-2.5 py-1.5",
                globalActive ? "border-border bg-muted" : "border-transparent hover:bg-accent",
              )}
            >
              {globalIndicator !== "none" && <StatusIcon status={GROUP_STATUS[globalIndicator]} />}
              <Star className="h-3 w-3 text-primary" />
              <span
                className={cn(
                  "whitespace-nowrap text-xs",
                  globalActive ? "text-foreground" : "text-muted-foreground",
                )}
              >
                Vista globale ({globalCount})
              </span>
            </div>
          )}
          {layout.groups.map((g) => {
            const isActive = !globalActive && g.workspaceId === layout.activeWorkspaceId;
            const members = tabs.filter((t) => g.order.includes(t.key));
            const name = members[0]?.workspace.name ?? g.workspaceId;
            const indicator = groupIndicator(members.map(statusOf));
            return (
              <div
                key={g.workspaceId}
                onClick={() => handleSelectWorkspace(g.workspaceId)}
                title={name}
                aria-label={`${name}: ${g.order.length} terminali`}
                className={cn(
                  "flex cursor-pointer select-none items-center gap-2 rounded-md border px-2.5 py-1.5",
                  isActive
                    ? "border-border bg-muted"
                    : "border-transparent hover:bg-accent",
                )}
              >
                {indicator !== "none" && <StatusIcon status={GROUP_STATUS[indicator]} />}
                <span
                  className={cn(
                    "whitespace-nowrap text-xs",
                    isActive ? "text-foreground" : "text-muted-foreground",
                  )}
                >
                  {name}
                </span>
                <span className="rounded bg-background px-1.5 text-[10px] text-muted-foreground">
                  {g.order.length}
                </span>
              </div>
            );
          })}
        </div>

        {/* Area dei pannelli. TUTTI i TerminalView di TUTTI i gruppi sono figli
            diretti di questo contenitore, con chiave stabile e nell'ordine di
            creazione: layout, riordino, cambio gruppo e maximize cambiano solo
            lo stile (rettangolo / display), mai la struttura del DOM, quindi
            nessun terminale viene smontato o spostato (ucciderebbe il pty). */}
        <div className="relative min-h-0 flex-1">
          {tabs.length === 0 && (
            <div className="flex h-full items-center justify-center p-8">
              <p className="text-center text-sm text-muted-foreground">
                Premi “+ Terminale” su un workspace per aprire una shell integrata.
              </p>
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
                focused={pl?.focused ?? false}
                label={
                  pl?.groupWorkspaceId === GLOBAL_ID
                    ? `${t.workspace.name} · ${tabLabel(t)}`
                    : tabLabel(t)
                }
                status={statusOf(t)}
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
      </div>

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
      />
    </div>
  );
}
