import { useState } from "react";
import { Pencil, Star, Trash2 } from "lucide-react";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { api } from "../../lib/tauri";
import type { BuiltinLaunch, Launch, Workspace } from "../../lib/tauri";
import {
  LAUNCH_COMMAND_MAX,
  LAUNCH_NAME_MAX,
  SHELL_LAUNCH_ID,
  resolveDefaultLaunch,
  sanitizeLaunchCommand,
} from "../../lib/launch";
import { cn } from "../../lib/utils";
import {
  BuiltinBadge,
  Field,
  Section,
  SettingsPage,
  selectClass,
  useWorkspaceOp,
} from "./shared";

interface Props {
  workspace: Workspace;
  builtins: BuiltinLaunch[];
  globalLaunch: string | null;
  onWorkspaceUpdated: (ws: Workspace) => void;
}

/** Workspace › Avvii: avvio predefinito, avvii integrati e personalizzati. */
export default function WorkspaceLaunchesPage({
  workspace,
  builtins,
  globalLaunch,
  onWorkspaceUpdated,
}: Props) {
  const { run, saving, error, setError } = useWorkspaceOp(onWorkspaceUpdated);

  // Form di aggiunta.
  const [newName, setNewName] = useState("");
  const [newCommand, setNewCommand] = useState("");
  const [newDefault, setNewDefault] = useState(false);

  // Modifica inline di un avvio esistente.
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState("");
  const [editCommand, setEditCommand] = useState("");

  const own = workspace.default_launch ?? null;
  // Il valore del select: un id stantio equivale a "eredita".
  const known = [
    SHELL_LAUNCH_ID,
    ...builtins.map((b) => b.id),
    ...workspace.launches.map((l) => l.id),
  ];
  const selectValue = own !== null && known.includes(own) ? own : "";
  const inherited = builtins.find((b) => b.id === globalLaunch);
  const effectiveId = resolveDefaultLaunch(workspace, builtins, globalLaunch)?.id;

  const setDefault = (launchId: string | null) =>
    void run(() => api.setDefaultLaunch(workspace.id, launchId));

  // Stella: se è già il predefinito del workspace torna a ereditare.
  const toggleDefault = (id: string) => setDefault(own === id ? null : id);

  const addLaunch = async () => {
    const created = await run(() =>
      api.addLaunch(workspace.id, newName, sanitizeLaunchCommand(newCommand), newDefault),
    );
    if (created) {
      setNewName("");
      setNewCommand("");
      setNewDefault(false);
    }
  };

  const startEdit = (l: Launch) => {
    setError(null);
    setEditingId(l.id);
    setEditName(l.name);
    setEditCommand(l.command);
  };

  const saveEdit = async (l: Launch) => {
    const saved = await run(() =>
      api.updateLaunch(workspace.id, l.id, editName, sanitizeLaunchCommand(editCommand)),
    );
    if (saved) setEditingId(null);
  };

  const canAdd =
    !saving && newName.trim() !== "" && sanitizeLaunchCommand(newCommand) !== "";

  const starButton = (id: string) => {
    const on = own === id;
    const label = on ? "Rimuovi predefinito del workspace" : "Imposta predefinito del workspace";
    return (
      <Button
        size="icon"
        variant="ghost"
        className="h-7 w-7"
        disabled={saving}
        title={label}
        aria-label={label}
        aria-pressed={on}
        onClick={() => toggleDefault(id)}
      >
        <Star className={cn(on && "fill-current")} />
      </Button>
    );
  };

  const defaultTag = (id: string) =>
    effectiveId === id && (
      <span className="text-[10px] font-normal text-primary">
        predefinito{own === id ? "" : " (ereditato)"}
      </span>
    );

  return (
    <SettingsPage
      title="Avvii"
      description="Cosa parte con “+ Terminale” in questo workspace e quali avvii compaiono nel suo menu “⋯”."
    >
      <Section>
        <Field
          label="Avvio predefinito"
          help="Quello che parte con “+ Terminale”. Eredita = usa la scelta delle impostazioni generali."
        >
          <select
            value={selectValue}
            disabled={saving}
            onChange={(e) => setDefault(e.target.value === "" ? null : e.target.value)}
            className={selectClass}
          >
            <option value="">
              Eredita dalle impostazioni generali ({inherited?.name ?? "Shell vuota"})
            </option>
            <option value={SHELL_LAUNCH_ID}>Shell vuota</option>
            {builtins.map((b) => (
              <option key={b.id} value={b.id}>
                {b.name}
              </option>
            ))}
            {workspace.launches.map((l) => (
              <option key={l.id} value={l.id}>
                {l.name}
              </option>
            ))}
          </select>
        </Field>
      </Section>

      <Section>
        <p className="text-xs font-semibold">Avvii del workspace</p>
        <div className="flex flex-col gap-2">
          {builtins.map((b) => (
            <div
              key={b.id}
              className="flex items-center gap-2 rounded-md border bg-background/40 p-2.5"
            >
              <div className="min-w-0 flex-1">
                <p className="flex items-center gap-2 text-[13px] font-semibold">
                  <span className="truncate">{b.name}</span>
                  <BuiltinBadge />
                  {defaultTag(b.id)}
                </p>
                <code
                  className="block truncate font-mono text-[11px] text-muted-foreground"
                  title={b.command}
                >
                  {b.command}
                </code>
              </div>
              {starButton(b.id)}
            </div>
          ))}
          {workspace.launches.length === 0 && (
            <p className="text-xs text-muted-foreground">
              Nessun avvio personalizzato. Aggiungine uno qui sotto.
            </p>
          )}
          {workspace.launches.map((l) =>
            editingId === l.id ? (
              <div
                key={l.id}
                className="flex flex-col gap-2 rounded-md border bg-background/40 p-2.5"
              >
                <Input
                  value={editName}
                  onChange={(e) => setEditName(e.target.value)}
                  placeholder="Nome"
                  maxLength={LAUNCH_NAME_MAX}
                  className="h-8 text-[13px]"
                  autoFocus
                />
                <Input
                  value={editCommand}
                  onChange={(e) => setEditCommand(e.target.value)}
                  placeholder="Comando"
                  maxLength={LAUNCH_COMMAND_MAX}
                  className="h-8 font-mono text-[12px]"
                />
                <div className="flex flex-row gap-1.5">
                  <Button size="sm" disabled={saving} onClick={() => void saveEdit(l)}>
                    Salva
                  </Button>
                  <Button size="sm" variant="secondary" onClick={() => setEditingId(null)}>
                    Annulla
                  </Button>
                </div>
              </div>
            ) : (
              <div
                key={l.id}
                className="flex items-center gap-2 rounded-md border bg-background/40 p-2.5"
              >
                <div className="min-w-0 flex-1">
                  <p className="flex items-center gap-2 text-[13px] font-semibold">
                    <span className="truncate">{l.name}</span>
                    {defaultTag(l.id)}
                  </p>
                  <code
                    className="block truncate font-mono text-[11px] text-muted-foreground"
                    title={l.command}
                  >
                    {l.command}
                  </code>
                </div>
                {starButton(l.id)}
                <Button
                  size="icon"
                  variant="ghost"
                  className="h-7 w-7"
                  title="Modifica"
                  aria-label="Modifica"
                  onClick={() => startEdit(l)}
                >
                  <Pencil />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  className="h-7 w-7"
                  disabled={saving}
                  title="Elimina"
                  aria-label="Elimina"
                  onClick={() => void run(() => api.removeLaunch(workspace.id, l.id))}
                >
                  <Trash2 />
                </Button>
              </div>
            ),
          )}
        </div>
      </Section>

      <Section>
        <p className="text-xs font-semibold">Nuovo avvio</p>
        <Input
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          placeholder="Nome (es. Dev server)"
          maxLength={LAUNCH_NAME_MAX}
          className="h-8 text-[13px]"
        />
        <Input
          value={newCommand}
          onChange={(e) => setNewCommand(e.target.value)}
          placeholder="Comando (es. npm run dev)"
          maxLength={LAUNCH_COMMAND_MAX}
          className="h-8 font-mono text-[12px]"
        />
        <label className="flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={newDefault}
            onChange={(e) => setNewDefault(e.target.checked)}
            className="h-3.5 w-3.5 accent-primary"
          />
          Predefinito del workspace
        </label>
        <div>
          <Button size="sm" disabled={!canAdd} onClick={() => void addLaunch()}>
            Aggiungi
          </Button>
        </div>
      </Section>

      {error && <p className="text-xs text-destructive">{error}</p>}
    </SettingsPage>
  );
}
