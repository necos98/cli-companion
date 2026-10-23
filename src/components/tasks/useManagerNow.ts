import { useEffect, useState } from "react";

/** Orologio condiviso del manager: un solo timer (~30 s) per tempi trascorsi e "x min fa". */
export function useManagerNow(intervalMs = 30_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);
  return now;
}
