import { memo } from "react";
import { Ellipsis } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "../ui/dropdown-menu";
import type { Workspace } from "../../lib/tauri";
import { cn } from "../../lib/utils";
import LaunchItems from "./LaunchItems";
import { changeLocation, manageLaunches } from "./actions";
import { sidebarStore } from "./sidebarState";
import { ICON_BTN, MENU_CONTENT, MENU_ITEM } from "./styles";

/** Menu ⋯ (non modale: un menu modale che apre un Dialog dal suo onSelect
    lascia `pointer-events: none` bloccato sul body). */
function WorkspaceMenu({ ws }: { ws: Workspace }) {
  return (
    <DropdownMenu modal={false}>
      <DropdownMenuTrigger asChild>
        <button type="button" className={ICON_BTN} title="Altre azioni" aria-label="Altre azioni">
          <Ellipsis />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        className={MENU_CONTENT}
        // Niente ritorno del focus al trigger: la rinomina inline deve poter
        // prendere il focus sull'input.
        onCloseAutoFocus={(e) => e.preventDefault()}
      >
        <LaunchItems ws={ws} />
        <DropdownMenuSeparator className="mx-1.5 bg-border" />
        <DropdownMenuItem onSelect={() => manageLaunches(ws)} className={MENU_ITEM}>
          Gestisci avvii…
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => sidebarStore.startEdit(ws.id, ws.name)} className={MENU_ITEM}>
          Rinomina
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => changeLocation(ws)} className={MENU_ITEM}>
          Cambia cartella…
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => sidebarStore.askArchive(ws.id)} className={MENU_ITEM}>
          Archivia
        </DropdownMenuItem>
        <DropdownMenuItem
          className={cn(MENU_ITEM, "text-destructive focus:text-destructive")}
          onSelect={() => sidebarStore.askRemove(ws.id)}
        >
          Elimina
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export default memo(WorkspaceMenu);
