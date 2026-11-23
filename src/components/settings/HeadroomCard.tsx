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
  const [executable, setExecutable] = useState(settings.executable);
  const [checking, setChecking] = useState(false);
  const [starting, setStarting] = useState(false);
  const busy = checking || starting;

  // La porta salvata (o riletta dal backend) vince sul testo in modifica.
  useEffect(() => setPort(String(settings.port)), [settings.port]);

  useEffect(() => setExecutable(settings.executable), [settings.executable]);

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

  const startProxy = () => {
    setStarting(true);
    setError(null);
    api
      .headroomStartProxy()
      .then(setStatus)
      .catch((e) => setError(errorMessage(e)))
      .finally(() => setStarting(false));
  };

  // Il proxy carica il modello e risponde dopo circa un minuto: finché sta partendo si rilegge lo stato.
  const waiting = starting || !!status?.starting;
  useEffect(() => {
    if (!waiting) return;
    const id = setInterval(() => {
      api
        .headroomStatus()
        .then(setStatus)
        .catch(() => {});
    }, 3000);
    return () => clearInterval(id);
  }, [waiting]);

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
    else void save({ ...settings, port: value });
  };

  const commitExecutable = () => {
    const value = executable.trim();
    if (value === settings.executable) setExecutable(settings.executable);
    else void save({ ...settings, executable: value });
  };

  return (
    <Section>
      <p className="text-xs font-semibold">Headroom</p>

      <label className="flex items-center gap-2 text-[13px]">
        <input
          type="checkbox"
          checked={settings.enabled}
          onChange={(e) => void save({ ...settings, enabled: e.target.checked })}
          className="h-3.5 w-3.5 accent-primary"
        />
        Usa Headroom per Claude Code
      </label>

      <label className="flex items-center gap-2 text-[13px]">
        <input
          type="checkbox"
          checked={settings.auto_start}
          onChange={(e) => void save({ ...settings, auto_start: e.target.checked })}
          className="h-3.5 w-3.5 accent-primary"
        />
        Avvia il proxy automaticamente
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

      <div className="flex items-center gap-2">
        <label htmlFor="headroom-executable" className="text-xs">
          Eseguibile
        </label>
        <Input
          id="headroom-executable"
          value={executable}
          placeholder="headroom"
          onChange={(e) => setExecutable(e.target.value)}
          onBlur={commitExecutable}
          onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
          className="h-8 flex-1"
        />
      </div>

      {error && <p className="whitespace-pre-wrap text-xs text-destructive">{error}</p>}

      {settings.enabled && (
        <div className="flex items-center gap-2 text-xs">
          {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          {status && !busy && <StatusText status={status} />}
          <Button size="sm" variant="outline" disabled={busy} onClick={verify}>
            Verifica
          </Button>
          {status && !status.reachable && !status.starting && (
            <Button size="sm" variant="outline" disabled={busy} onClick={startProxy}>
              Avvia proxy
            </Button>
          )}
          <Button size="sm" variant="outline" onClick={openHeadroomDashboard}>
            Apri dashboard
          </Button>
        </div>
      )}

      <p className="text-[11px] text-muted-foreground">
        Solo per le sessioni Claude Code su Windows. Con l&apos;avvio automatico l&apos;app lancia{" "}
        <code className="font-mono">headroom proxy</code> (senza rate limit) e lo ferma all&apos;uscita o quando
        disattivi Headroom, senza mai toccare un proxy avviato altrove. L&apos;avvio richiede circa un minuto: i worker
        lo aspettano, i terminali interattivi aperti nel frattempo partono senza; il log è in{" "}
        <code className="font-mono">logs/headroom-proxy.log</code> nella cartella dati dell&apos;app. Senza avvio
        automatico avvia il proxy tu; se non risponde le sessioni partono senza. La dashboard si apre in una finestra
        dedicata dalla barra dei mondi o dal piede della sidebar.
      </p>
    </Section>
  );
}

function StatusText({ status }: { status: HeadroomStatus }) {
  if (status.starting) return <span>Proxy in avvio su {status.url}… (può richiedere circa un minuto)</span>;
  if (!status.reachable) return <span className="text-muted-foreground">Proxy non raggiungibile</span>;
  return (
    <span>
      {status.managed ? `Proxy avviato dall'app su ${status.url}` : `Proxy esterno in ascolto su ${status.url}`}
    </span>
  );
}
