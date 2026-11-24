import { memo, useCallback } from "react";
import { useStore } from "../../hooks/useStore";
import { useOverlayShown, useViewWorkspaceId } from "../../hooks/useView";
import { openIntake } from "../../app/actions/tabs";
import { focusTerminal, openManager, selectWorkspace } from "../../app/actions/view";
import { storeBoard } from "../../app/boards";
import { GLOBAL_ID } from "../../lib/layout";
import { botImageResolver, configStore, shownBot } from "../../stores/config";
import { boardsStore } from "../../stores/boards";
import { layoutStore } from "../../stores/layout";
import { tabsStore } from "../../stores/tabs";
import { visibleWorkspaces, workspacesStore } from "../../stores/workspaces";
import GlobalTaskPanel from "../tasks/GlobalTaskPanel";
import TaskPanel from "../tasks/TaskPanel";
import { useTaskPanelCollapsed } from "../tasks/useTaskPanelCollapsed";
import { useInertRef } from "./useInertRef";

/**
 * Bot di un tab per il pannello Task (nome e immagine, solo con la funzione
 * attiva). Le funzioni cambiano quando cambiano i tab o le impostazioni dei
 * bot, così il pannello si ridisegna da solo.
 */
function useBotLookups() {
  const tabs = useStore(tabsStore);
  const bots = useStore(configStore, (s) => s.botSettings);
  const botOf = useCallback(
    (tabId: string) => {
      const tab = tabs.find((t) => t.key === tabId);
      return tab && shownBot(bots, tab.bot);
    },
    [tabs, bots],
  );
  const botImageOf = useCallback(
    (tabId: string) => {
      const tab = tabs.find((t) => t.key === tabId);
      return tab && bots.enabled ? botImageResolver(bots)(tab.bot) : undefined;
    },
    [tabs, bots],
  );
  return { botOf, botImageOf };
}

/**
 * Pannello Task (a sinistra della griglia): del workspace mostrato, o della
 * vista Globale. Il contenitore resta sempre al suo posto, prima dell'area dei
 * pannelli: cambiare pannello non tocca i terminali.
 */
function TaskPanelArea() {
  const viewId = useViewWorkspaceId();
  const globalActive = useStore(layoutStore, (s) => s.layout.activeView === "global");
  const visible = useStore(workspacesStore, visibleWorkspaces);
  const boards = useStore(boardsStore);
  const overlayShown = useOverlayShown();
  const inertRef = useInertRef(overlayShown);
  const taskPanel = useTaskPanelCollapsed();
  const { botOf, botImageOf } = useBotLookups();

  // Workspace del pannello Task: quello mostrato (anche senza terminali).
  const panelWorkspace = visible.find((w) => w.id === viewId);

  return (
    // Il pannello è una scheda arrotondata nello sfondo dell'area terminali:
    // stesso margine esterno dei pannelli (6px), e 8px di distanza dalla
    // griglia (2px qui + 6px di margine della griglia).
    <div
      className={`flex flex-shrink-0 ${
        panelWorkspace || globalActive ? "bg-pane-area py-[6px] pl-[6px] pr-[2px]" : ""
      }`}
      aria-hidden={overlayShown || undefined}
      // Coperto dal Tasks Manager: non raggiungibile con Tab (solo attributo).
      ref={inertRef}
    >
      {panelWorkspace ? (
        <TaskPanel
          key={panelWorkspace.id}
          workspace={panelWorkspace}
          board={boards.boards[panelWorkspace.id] ?? null}
          loadError={boards.errors[panelWorkspace.id] ?? null}
          collapsed={taskPanel.isCollapsed(panelWorkspace.id)}
          onToggleCollapsed={() => taskPanel.toggle(panelWorkspace.id)}
          onBoard={storeBoard}
          onOpenManager={(taskId) => openManager(panelWorkspace.id, taskId ? { taskId } : undefined)}
          onOpenIntake={() => openIntake(panelWorkspace.id)}
          onNewTask={() => openManager(panelWorkspace.id, { newTask: true })}
          onFocusTab={focusTerminal}
          botOf={botOf}
          botImageOf={botImageOf}
        />
      ) : globalActive ? (
        <GlobalTaskPanel
          workspaces={visible}
          boards={boards.boards}
          onOpenWorkspace={selectWorkspace}
          onFocusTab={focusTerminal}
          botOf={botOf}
          botImageOf={botImageOf}
          collapsed={taskPanel.isCollapsed(GLOBAL_ID)}
          onToggleCollapsed={() => taskPanel.toggle(GLOBAL_ID)}
        />
      ) : null}
    </div>
  );
}

export default memo(TaskPanelArea);
