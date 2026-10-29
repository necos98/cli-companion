import { ImageOff, ImagePlus, ImageUp, X } from "lucide-react";
import type { ReactNode } from "react";
import type { BotImage } from "../../lib/bots";
import BotAvatar from "../BotAvatar";

/** Stile dei bottoncini a icona della riga. */
const ICON_BUTTON =
  "flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-[5px] text-muted-foreground transition-colors hover:bg-surface-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50";

interface IconButtonProps {
  label: string;
  disabled: boolean;
  onClick: () => void;
  children: ReactNode;
}

function IconButton({ label, disabled, onClick, children }: IconButtonProps) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      title={label}
      aria-label={label}
      className={ICON_BUTTON}
    >
      {children}
    </button>
  );
}

interface Props {
  name: string;
  /** Immagine salvata per il nome, se c'e'. */
  image?: BotImage;
  disabled: boolean;
  onPickImage: (name: string) => void;
  onRemoveImage: (name: string) => void;
  onRemove: (name: string) => void;
}

/** Riga della pool: avatar, nome e azioni (immagine, rimozione del nome). */
export default function BotNameRow({ name, image, disabled, onPickImage, onRemoveImage, onRemove }: Props) {
  return (
    <li className="flex min-w-0 items-center gap-2 rounded-md border bg-background/40 py-1 pl-2 pr-1">
      <BotAvatar name={name} size={18} image={image} />
      <span className="min-w-0 flex-1 truncate text-[13px]" title={name}>
        {name}
      </span>
      {image ? (
        <>
          <IconButton label={`Cambia immagine di ${name}`} disabled={disabled} onClick={() => onPickImage(name)}>
            <ImageUp className="h-3.5 w-3.5" />
          </IconButton>
          <IconButton label={`Rimuovi immagine di ${name}`} disabled={disabled} onClick={() => onRemoveImage(name)}>
            <ImageOff className="h-3.5 w-3.5" />
          </IconButton>
        </>
      ) : (
        <IconButton label={`Scegli immagine per ${name}…`} disabled={disabled} onClick={() => onPickImage(name)}>
          <ImagePlus className="h-3.5 w-3.5" />
        </IconButton>
      )}
      <IconButton label={`Rimuovi ${name}`} disabled={disabled} onClick={() => onRemove(name)}>
        <X className="h-3.5 w-3.5" />
      </IconButton>
    </li>
  );
}
