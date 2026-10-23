import { useEffect, useRef, useState } from "react";
import { Loader2, Plus, Trash2 } from "lucide-react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { api, onScheduleState } from "../lib/tauri";
import {
  MAX_WINDOWS,
  addWindow,
  crossesMidnight,
  normalizeSchedule,
  removeWindow,
  sameSchedule,
  scheduleError,
  scheduleStatusText,
  updateWindow,
  windowDescription,
  windowError,
} from "../lib/schedule";
import type { Schedule, ScheduleState } from "../lib/schedule";

/** Impostazioni globali dell'esecuzione automatica (fasce orarie, standby). */
export default function ScheduleSettings() {
  const [state, setState] = useState<ScheduleState | null>(null);
  const [draft, setDraft] = useState<Schedule | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  // La bozza segue gli eventi del backend solo finché non è stata modificata.
  const draftRef = useRef<Schedule | null>(null);
  const stateRef = useRef<ScheduleState | null>(null);
  draftRef.current = draft;
  stateRef.current = state;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    api
      .scheduleGet()
      .then((s) => {
        if (cancelled) return;
        setState(s);
        setDraft(s.schedule);
      })
      .catch((e) => !cancelled && setLoadError(String(e)));
    onScheduleState((s) => {
      const dirty =
        draftRef.current !== null &&
        stateRef.current !== null &&
        !sameSchedule(draftRef.current, stateRef.current.schedule);
      setState(s);
      if (!dirty) setDraft(s.schedule);
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
  }, []);

  if (loadError) return <p className="text-xs text-destructive">{loadError}</p>;
  if (!state || !draft) {
    return (
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" />
        Caricamento…
      </div>
    );
  }

  const problem = scheduleError(draft);
  const dirty = !sameSchedule(draft, state.schedule);

  const save = async () => {
    setError(null);
    setSaving(true);
    try {
      const next = await api.scheduleSet(normalizeSchedule(draft));
      setState(next);
      setDraft(next.schedule);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <p className="text-[11px] text-muted-foreground">
        I workspace che rispettano le fasce orarie (Impostazioni › Worker) fanno
        partire i task in coda solo dentro le fasce; fuori non parte nessun
        nuovo task e quelli in corso finiscono.
      </p>

      <label className="flex items-center gap-2 text-[13px] font-medium">
        <input
          type="checkbox"
          checked={draft.enabled}
          onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
          className="h-3.5 w-3.5 accent-primary"
        />
        Attiva l’esecuzione automatica
      </label>

      <div className="flex flex-col gap-2 rounded-lg border p-2.5">
        <div className="flex items-center justify-between">
          <p className="text-xs font-semibold">Fasce orarie</p>
          <Button
            size="sm"
            variant="secondary"
            className="h-7 gap-1 px-2 text-[11px]"
            disabled={draft.windows.length >= MAX_WINDOWS}
            onClick={() => setDraft({ ...draft, windows: addWindow(draft.windows) })}
          >
            <Plus className="!size-3" />
            Aggiungi
          </Button>
        </div>

        {draft.windows.length === 0 && (
          <p className="text-xs text-muted-foreground">Nessuna fascia configurata.</p>
        )}
        {draft.windows.map((w, i) => {
          const err = windowError(w);
          return (
            <div key={i} className="flex flex-col gap-1">
              <div className="flex items-center gap-2">
                <span className="w-12 text-[11px] text-muted-foreground">Dalle</span>
                <Input
                  type="time"
                  value={w.start}
                  onChange={(e) =>
                    setDraft({ ...draft, windows: updateWindow(draft.windows, i, { start: e.target.value }) })
                  }
                  className="h-8 w-28 font-mono text-[12px]"
                  aria-label={`Inizio della fascia ${i + 1}`}
                />
                <span className="text-[11px] text-muted-foreground">alle</span>
                <Input
                  type="time"
                  value={w.end}
                  onChange={(e) =>
                    setDraft({ ...draft, windows: updateWindow(draft.windows, i, { end: e.target.value }) })
                  }
                  className="h-8 w-28 font-mono text-[12px]"
                  aria-label={`Fine della fascia ${i + 1}`}
                />
                <Button
                  size="icon"
                  variant="ghost"
                  className="ml-auto h-7 w-7"
                  title="Rimuovi la fascia"
                  aria-label={`Rimuovi la fascia ${i + 1}`}
                  onClick={() => setDraft({ ...draft, windows: removeWindow(draft.windows, i) })}
                >
                  <Trash2 />
                </Button>
              </div>
              {err ? (
                <p className="pl-14 text-[11px] text-destructive">{err}</p>
              ) : (
                crossesMidnight(w) && (
                  <p className="pl-14 text-[11px] text-muted-foreground">
                    {windowDescription(w)}: attraversa la mezzanotte.
                  </p>
                )
              )}
            </div>
          );
        })}
        <p className="text-[11px] text-muted-foreground">
          Ora locale, massimo {MAX_WINDOWS} fasce. Se la fine è prima dell’inizio
          la fascia prosegue oltre la mezzanotte (es. 20:00–13:00 = dalla sera alle
          13:00 del giorno dopo). Le fasce sovrapposte valgono insieme.
        </p>
      </div>

      <div>
        <label className="flex items-center gap-2 text-[13px]">
          <input
            type="checkbox"
            checked={draft.keep_awake}
            onChange={(e) => setDraft({ ...draft, keep_awake: e.target.checked })}
            className="h-3.5 w-3.5 accent-primary"
          />
          Impedisci lo standby
        </label>
        <p className="mt-1 pl-5 text-[11px] text-muted-foreground">
          Il computer non va in standby finché c’è almeno un task in corso in
          un workspace.
        </p>
      </div>

      <p className="rounded-md bg-surface-2 px-2.5 py-1.5 text-xs" aria-live="polite">
        {scheduleStatusText(state)}
      </p>

      {(error ?? problem) && dirty && (
        <p className="text-xs text-destructive">{error ?? problem}</p>
      )}

      <div>
        <Button size="sm" disabled={saving || !dirty || problem !== null} onClick={() => void save()}>
          {saving ? "Salvataggio…" : "Salva"}
        </Button>
      </div>
    </div>
  );
}
