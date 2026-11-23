import { Building2, Gauge, MessagesSquare, SlidersHorizontal, SquareTerminal } from "lucide-react";
import { openHeadroomDashboard } from "../../app/actions/headroom";
import { openSettings } from "../../app/actions/settings";
import { showHub, showTerminals, toggleOffice } from "../../app/actions/view";
import { useHubWaitingTotal } from "../../hooks/useHubChannels";
import { waitingText } from "../../lib/hub";
import { useStore } from "../../hooks/useStore";
import { useActiveWorld, useHeadroomEnabled, useOfficeShown } from "../../hooks/useView";
import { configStore } from "../../stores/config";
import { uiStore } from "../../stores/ui";
import { Button } from "../ui/button";
import UpdateDot from "../UpdateDot";
import RailItem from "./RailItem";

/**
 * Barra dei mondi (64px, solo con l'Agents HUB acceso): Terminali, Agents HUB
 * con i thread in attesa, Ufficio (se i bot sono attivi), poi Impostazioni e
 * versione. Sempre visibile, qualunque mondo sia aperto.
 */
export default function WorldRail() {
  const world = useActiveWorld();
  const officeShown = useOfficeShown();
  const headroomEnabled = useHeadroomEnabled();
  const waiting = useHubWaitingTotal();
  const botsEnabled = useStore(configStore, (s) => s.botSettings.enabled);
  const appVersion = useStore(configStore, (s) => s.appVersion);
  const updateAvailable = useStore(uiStore, (s) => s.updateAvailable);

  return (
    <nav aria-label="Mondi" className="flex w-16 flex-shrink-0 flex-col items-center gap-1 border-r bg-base py-2">
      <RailItem
        icon={SquareTerminal}
        label="Terminali"
        title="Terminali (Ctrl+Maiusc+H)"
        active={world === "terminals" && !officeShown}
        onClick={showTerminals}
      />
      <RailItem
        icon={MessagesSquare}
        label="HUB"
        title="Agents HUB (Ctrl+Maiusc+H)"
        ariaLabel={waiting > 0 ? `Agents HUB, ${waitingText(waiting)}` : undefined}
        active={world === "hub"}
        badge={waiting}
        onClick={showHub}
      />
      {botsEnabled && (
        <RailItem
          icon={Building2}
          label="Ufficio"
          title="Ufficio (Ctrl+Maiusc+O)"
          active={officeShown}
          onClick={toggleOffice}
        />
      )}
      {headroomEnabled && (
        <RailItem
          icon={Gauge}
          label="Headroom"
          title="Dashboard di Headroom"
          active={false}
          onClick={openHeadroomDashboard}
        />
      )}
      <span className="flex-1" />
      <Button
        variant="ghost"
        size="icon"
        onClick={openSettings}
        aria-label="Impostazioni"
        title="Impostazioni"
        className="relative size-10 text-text-secondary hover:text-foreground"
      >
        <SlidersHorizontal className="!size-[18px]" />
        {updateAvailable && <UpdateDot className="absolute right-2 top-2" />}
      </Button>
      {appVersion && <span className="font-mono text-[11px] text-faint">v{appVersion}</span>}
    </nav>
  );
}
