import { useCallback, useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import type { HeadroomSettings } from "../../bindings/HeadroomSettings";
import type { HeadroomStatus } from "../../bindings/HeadroomStatus";
import { openHeadroomDashboard, saveHeadroomSettings } from "../../app/actions/headroom";
import { errorMessage } from "../../lib/errors";
import { api } from "../../lib/tauri";
import { useStore } from "../../hooks/useStore";
import { headroomStore } from "../../stores/headroom";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Section } from "./shared";

/** Generale › Integrazioni: Headroom, il proxy locale che comprime il contesto di Claude Code. */
export default function HeadroomCard() {
  const settings = useStore(headroomStore, (s) => s.settings);
  const [port, setPort] = useState(String(settings.port));
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<HeadroomStatus | null>(null);
  const [checking, setChecking] = useState(false);

  // La porta salvata (o riletta dal backend) vince sul testo in modifica.
  useEffect(() => setPort(String(settings.port)), [settings.port]);

  const verify = useCallback(() => {
    setChecking(true);
    api
      .headroomStatus()
      .then(setStatus)
      .catch((e) => setError(errorMessage(e)))
      .finally(() => setChecking(false));
  }, []);

  // Con Headroom acceso si controlla all'apertura e a ogni cambio di porta.
  useEffect(() => {
    if (settings.enabled) verify();
    else setStatus(null);
  }, [settings.enabled, settings.port, verify]);

  const save = async (next: HeadroomSettings) => {
    try {
      await saveHeadroomSettings(next);
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
      setPort(String(settings.port));
    }
  };

  const commitPort = () => {
    const text = port.trim();
    const value = Number(text);
    if (text === "" || value === settings.port) setPort(String(settings.port));
    else void save({ enabled: settings.enabled, port: value });
  };

  return (
    <Section>
      <p className="text-xs font-semibold">Headroom</p>

      <label className="flex items-center gap-2 text-[13px]">
        <input
          type="checkbox"
          checked={settings.enabled}
          onChange={(e) => void save({ enabled: e.target.checked, port: settings.port })}
          className="h-3.5 w-3.5 accent-primary"
        />
        Usa Headroom per Claude Code
      </label>

      <div className="flex items-center gap-2">
        <label htmlFor="headroom-port" className="text-xs">
          Porta
        </label>
        <Input
          id="headroom-port"
          type="number"
          min={1}
          max={65535}
          value={port}
          onChange={(e) => setPort(e.target.value)}
          onBlur={commitPort}
          onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
          className="h-8 w-28"
        />
      </div>

      {error && <p className="whitespace-pre-wrap text-xs text-destructive">{error}</p>}

      {settings.enabled && (
        <div className="flex items-center gap-2 text-xs">
          {checking && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          {status && !checking && (
            <span className={status.reachable ? "text-foreground" : "text-muted-foreground"}>
              {status.reachable ? `Proxy in ascolto su ${status.url}` : "Proxy non raggiungibile"}
            </span>
          )}
          <Button size="sm" variant="outline" disabled={checking} onClick={verify}>
            Verifica
          </Button>
          <Button size="sm" variant="outline" onClick={openHeadroomDashboard}>
            Apri dashboard
          </Button>
        </div>
      )}

      <p className="text-[11px] text-muted-foreground">
        Solo per le sessioni Claude Code su Windows. Avvia tu il proxy (
        <code className="font-mono">headroom proxy</code> o <code className="font-mono">headroom deploy</code>); se non
        risponde le sessioni partono senza. Con Headroom acceso la sua dashboard si apre in una finestra dedicata dalla
        barra dei mondi o dal piede della sidebar.
      </p>
    </Section>
  );
}
