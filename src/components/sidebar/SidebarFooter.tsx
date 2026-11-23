import { Gauge, SlidersHorizontal } from "lucide-react";
import { Button } from "../ui/button";
import OfficeButton from "../office/OfficeButton";
import UpdateDot from "../UpdateDot";
import { useStore } from "../../hooks/useStore";
import { useHeadroomEnabled, useOfficeShown } from "../../hooks/useView";
import { openHeadroomDashboard } from "../../app/actions/headroom";
import { openSettings } from "../../app/actions/settings";
import { toggleOffice } from "../../app/actions/view";
import { configStore } from "../../stores/config";
import { uiStore } from "../../stores/ui";

/** Piede della sidebar (senza la barra dei mondi): Impostazioni (con il pallino se c'è un aggiornamento), Ufficio e versione. */
export default function SidebarFooter() {
  const updateAvailable = useStore(uiStore, (s) => s.updateAvailable);
  const botsEnabled = useStore(configStore, (s) => s.botSettings.enabled);
  const appVersion = useStore(configStore, (s) => s.appVersion);
  const officeShown = useOfficeShown();
  const headroomEnabled = useHeadroomEnabled();

  return (
    <div className="flex items-center gap-2 border-t px-3 py-2.5">
      <Button
        variant="ghost"
        onClick={openSettings}
        className="h-auto flex-1 justify-start gap-2 px-2 py-[7px] text-[13px] font-normal text-text-secondary hover:text-foreground"
      >
        <SlidersHorizontal className="!size-[15px]" />
        Impostazioni
        {updateAvailable && <UpdateDot />}
      </Button>
      {headroomEnabled && (
        <Button
          type="button"
          variant="ghost"
          size="icon"
          onClick={openHeadroomDashboard}
          aria-label="Headroom"
          title="Dashboard di Headroom"
          className="h-8 w-8 flex-shrink-0 text-text-secondary hover:text-foreground"
        >
          <Gauge className="!size-[15px]" />
        </Button>
      )}
      {botsEnabled && <OfficeButton shown={officeShown} onToggle={toggleOffice} />}
      {appVersion && <span className="font-mono text-[11px] text-faint">v{appVersion}</span>}
    </div>
  );
}
