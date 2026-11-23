import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import { refreshIntegrations } from "../../app/actions/integrations";
import { errorMessage } from "../../lib/errors";
import { useStore } from "../../hooks/useStore";
import { configStore } from "../../stores/config";
import HeadroomCard from "./HeadroomCard";
import IntegrationCard from "./IntegrationCard";
import IntegrationsGeneral from "./IntegrationsGeneral";
import { SettingsPage } from "./shared";

/** Generale › Integrazioni: abilitazione, predefinita e pacchetto per ambiente dei coding agent. */
export default function IntegrationsPage() {
  const integrations = useStore(configStore, (s) => s.integrations);
  const [loadError, setLoadError] = useState<string | null>(null);

  // All'apertura si rilevano di nuovo gli ambienti (l'utente può aver installato un agente).
  useEffect(() => {
    refreshIntegrations()
      .then(() => setLoadError(null))
      .catch((e) => setLoadError(errorMessage(e)));
  }, []);

  return (
    <SettingsPage
      title="Integrazioni"
      description="I coding agent che cli-companion sa usare: abilita quelli che ti servono, scegli il predefinito e installa il pacchetto negli ambienti in cui li lanci."
    >
      {loadError && <p className="text-xs text-destructive">{loadError}</p>}
      {integrations.length === 0 && !loadError && (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
          Rilevamento ambienti…
        </div>
      )}
      {integrations.map((info) => (
        <IntegrationCard key={info.id} info={info} />
      ))}
      {integrations.length > 0 && <IntegrationsGeneral integrations={integrations} />}
      <HeadroomCard />
    </SettingsPage>
  );
}
