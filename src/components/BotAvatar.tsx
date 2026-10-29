import { BOT_GRID, botImageHref, botLook, botPixels } from "../lib/bots";
import type { BotImage, PixelTone } from "../lib/bots";
import { cn } from "../lib/utils";

interface Props {
  /** Nome del bot: da lui dipende tutto l'aspetto. */
  name: string;
  /** Lato in pixel (16-20 nelle intestazioni). */
  size?: number;
  /** Immagine del nome: sostituisce la testa generata. */
  image?: BotImage;
  className?: string;
}

export const DARK = "#0f172a";
export const LIGHT = "rgba(255, 255, 255, 0.6)";

/** Avatar pixel-art di un bot: generato dal nome, o l'immagine pixelata del nome se c'e'. */
export default function BotAvatar({ name, size = 18, image, className }: Props) {
  const look = botLook(name);
  const fill: Record<PixelTone, string> = { body: look.color, dark: DARK, light: LIGHT };
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${BOT_GRID} ${BOT_GRID}`}
      shapeRendering="crispEdges"
      role="img"
      aria-label={name}
      className={cn("flex-shrink-0", className)}
    >
      {image ? (
        <image
          href={botImageHref(image)}
          x={0}
          y={0}
          width={BOT_GRID}
          height={BOT_GRID}
          preserveAspectRatio="xMidYMid meet"
          style={{ imageRendering: "pixelated" }}
        />
      ) : (
        botPixels(look).map((r, i) => (
          <rect key={i} x={r.x} y={r.y} width={r.w} height={r.h} fill={fill[r.tone]} />
        ))
      )}
    </svg>
  );
}
