import "../office/office.css";
import type { BotImage } from "../../lib/bots";
import BotAvatar from "../BotAvatar";
import BotSprite from "../office/BotSprite";
import { Button } from "../ui/button";

/** Riquadro dello sprite nell'anteprima (px): stesso rapporto 16 x 24 dello sprite. */
const PREVIEW_W = 64;
const PREVIEW_H = 96;
const AVATAR_SIZE = 18;

interface Props {
  name: string;
  image: BotImage;
  /** Pixelazione o salvataggio in corso: i bottoni restano spenti. */
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/** Anteprima dell'immagine pixelata come sprite e come avatar, prima di salvarla. */
export default function BotImagePreview({ name, image, busy, onConfirm, onCancel }: Props) {
  return (
    <div className="flex items-center gap-4 rounded-md border bg-background/40 p-3">
      <div style={{ width: PREVIEW_W, height: PREVIEW_H }} className="flex-shrink-0">
        <BotSprite name={name} image={image} pose="standing" facing={1} intake={false} running={false} />
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="flex items-center gap-2">
          <BotAvatar name={name} size={AVATAR_SIZE} image={image} />
          <span className="min-w-0 truncate text-[13px] font-medium" title={name}>
            {name}
          </span>
        </div>
        <p className="text-[11px] text-muted-foreground">
          Così apparirà il bot nell’Ufficio e negli avatar. Funzionano meglio PNG con sfondo trasparente.
        </p>
        <div className="flex gap-2">
          <Button size="sm" disabled={busy} onClick={onConfirm}>
            Usa questa immagine
          </Button>
          <Button size="sm" variant="outline" disabled={busy} onClick={onCancel}>
            Annulla
          </Button>
        </div>
      </div>
    </div>
  );
}
