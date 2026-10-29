import { useState } from "react";
import type { FormEvent } from "react";
import { api } from "../../lib/tauri";
import type { BotSettings } from "../../lib/tauri";
import { BOT_NAME_MAX } from "../../lib/bots";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import BotImagePreview from "./BotImagePreview";
import BotNameRow from "./BotNameRow";
import { Field, Section, SettingsPage } from "./shared";
import { BOT_IMAGE_ACCEPT, useBotImagePicker } from "./useBotImagePicker";

interface Props {
  settings: BotSettings;
  onSettingsChange: (settings: BotSettings) => void;
}

/** Generale › Bot: nome e avatar di ogni sessione, e la pool da cui si pescano i nomi. */
export default function BotsPage({ settings, onSettingsChange }: Props) {
  const [newName, setNewName] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmingReset, setConfirmingReset] = useState(false);
  const picker = useBotImagePicker(setError);

  /** Salva subito; in caso di errore lo stato resta quello di prima. */
  const save = async (op: () => Promise<BotSettings>): Promise<boolean> => {
    setError(null);
    setSaving(true);
    try {
      onSettingsChange(await op());
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const setEnabled = (enabled: boolean) =>
    save(() => api.setBotSettings(enabled, settings.names));

  const removeName = (name: string) =>
    save(() =>
      api.setBotSettings(
        settings.enabled,
        settings.names.filter((n) => n !== name),
      ),
    );

  const removeImage = (name: string) => save(() => api.removeBotImage(name));

  const confirmImage = async () => {
    const { candidate } = picker;
    if (!candidate) return;
    const { name, image } = candidate;
    if (await save(() => api.setBotImage(name, image.png, image.color))) picker.cancel();
  };

  const addName = async (e: FormEvent) => {
    e.preventDefault();
    if (newName.trim() === "") return;
    const added = await save(() =>
      api.setBotSettings(settings.enabled, [...settings.names, newName]),
    );
    if (added) setNewName("");
  };

  const resetNames = async () => {
    setConfirmingReset(false);
    await save(() => api.resetBotNames());
  };

  return (
    <SettingsPage
      title="Bot"
      description="Ogni sessione è un bot con un nome e un piccolo avatar, per riconoscerla a colpo d’occhio."
    >
      <Section>
        <label className="flex items-center gap-2 text-[13px]">
          <input
            type="checkbox"
            checked={settings.enabled}
            disabled={saving}
            onChange={(e) => void setEnabled(e.target.checked)}
            className="h-3.5 w-3.5 accent-primary"
          />
          Mostra le sessioni come bot
        </label>
        <p className="-mt-1 text-[11px] text-muted-foreground">
          I nomi vengono assegnati comunque: riaccendendo la funzione compaiono subito.
        </p>
      </Section>

      <Section>
        <Field
          label="Nomi"
          help="Ogni nuova sessione prende a caso un nome non usato dalle sessioni aperte. Finiti i nomi, si aggiunge un numero (“Bolt 2”, che usa l’immagine di “Bolt”). A ogni nome puoi associare un’immagine: diventa il corpo del bot (funzionano meglio i PNG con sfondo trasparente)."
        >
          <form onSubmit={(e) => void addName(e)} className="flex gap-2">
            <Input
              value={newName}
              maxLength={BOT_NAME_MAX}
              disabled={saving}
              onChange={(e) => setNewName(e.target.value)}
              placeholder="Nuovo nome"
              aria-label="Nuovo nome"
              className="h-8"
            />
            <Button type="submit" size="sm" disabled={saving || newName.trim() === ""}>
              Aggiungi
            </Button>
          </form>
        </Field>
        {error && <p className="text-[11px] text-destructive">{error}</p>}
        <input
          ref={picker.inputRef}
          type="file"
          accept={BOT_IMAGE_ACCEPT}
          className="hidden"
          tabIndex={-1}
          aria-hidden="true"
          onChange={(e) => void picker.onFileChange(e)}
        />
        {picker.candidate && (
          <BotImagePreview
            name={picker.candidate.name}
            image={picker.candidate.image}
            busy={saving || picker.pixelating}
            onConfirm={() => void confirmImage()}
            onCancel={picker.cancel}
          />
        )}

        {settings.names.length === 0 ? (
          <p className="text-xs text-muted-foreground">
            Nessun nome: le sessioni si chiameranno “Bot 1”, “Bot 2”...
          </p>
        ) : (
          <ul className="grid grid-cols-2 gap-1.5">
            {settings.names.map((name) => (
              <BotNameRow
                key={name}
                name={name}
                image={settings.images[name]}
                disabled={saving || picker.pixelating}
                onPickImage={picker.pick}
                onRemoveImage={(n) => void removeImage(n)}
                onRemove={(n) => void removeName(n)}
              />
            ))}
          </ul>
        )}

        <div className="flex items-center gap-2">
          {confirmingReset ? (
            <>
              <span className="text-[11px] text-muted-foreground">
                Sostituire l’elenco con quello predefinito?
              </span>
              <Button size="sm" variant="destructive" disabled={saving} onClick={() => void resetNames()}>
                Ripristina
              </Button>
              <Button size="sm" variant="outline" onClick={() => setConfirmingReset(false)}>
                Annulla
              </Button>
            </>
          ) : (
            <Button
              size="sm"
              variant="outline"
              disabled={saving}
              onClick={() => setConfirmingReset(true)}
            >
              Ripristina nomi predefiniti
            </Button>
          )}
        </div>
      </Section>
    </SettingsPage>
  );
}
