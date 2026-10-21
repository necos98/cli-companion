import { useState } from "react";
import { api } from "../../lib/tauri";
import type { BuiltinLaunch } from "../../lib/tauri";
import { BuiltinBadge, Field, Section, SettingsPage, selectClass } from "./shared";

interface Props {
  builtins: BuiltinLaunch[];
  globalLaunch: string | null;
  onGlobalLaunchChange: (id: string | null) => void;
}

/** Generale › Avvii: avvio predefinito globale e avvii integrati. */
export default function GeneralLaunchesPage({
  builtins,
  globalLaunch,
  onGlobalLaunchChange,
}: Props) {
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const change = async (value: string) => {
    setError(null);
    setSaving(true);
    try {
      const s = await api.setGlobalDefaultLaunch(value === "" ? null : value);
      onGlobalLaunchChange(s.default_launch);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <SettingsPage
      title="Avvii"
      description="Cosa parte quando apri un nuovo terminale con “+ Terminale”."
    >
      <Section>
        <Field
          label="Avvio predefinito per tutti i workspace"
          help="Ogni workspace può sostituirlo con un proprio avvio predefinito."
        >
          <select
            value={builtins.some((b) => b.id === globalLaunch) ? (globalLaunch ?? "") : ""}
            disabled={saving}
            onChange={(e) => void change(e.target.value)}
            className={selectClass}
          >
            <option value="">Shell vuota</option>
            {builtins.map((b) => (
              <option key={b.id} value={b.id}>
                {b.name}
              </option>
            ))}
          </select>
        </Field>
        {error && <p className="text-[11px] text-destructive">{error}</p>}
      </Section>

      <Section>
        <div>
          <p className="text-xs font-semibold">Avvii integrati</p>
          <p className="mt-0.5 text-[11px] text-muted-foreground">
            Compaiono nel menu degli avvii di ogni workspace, prima di quelli
            personalizzati.
          </p>
        </div>
        {builtins.length === 0 && (
          <p className="text-xs text-muted-foreground">Nessun avvio integrato.</p>
        )}
        {builtins.map((b) => (
          <div key={b.id} className="min-w-0 rounded-md border bg-background/40 p-2.5">
            <p className="flex items-center gap-2 text-[13px] font-semibold">
              <span className="truncate">{b.name}</span>
              <BuiltinBadge />
            </p>
            <code
              className="block truncate font-mono text-[11px] text-muted-foreground"
              title={b.command}
            >
              {b.command}
            </code>
          </div>
        ))}
      </Section>
    </SettingsPage>
  );
}
