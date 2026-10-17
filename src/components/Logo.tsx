interface LogoProps {
  /** Lato in px (il logo è quadrato). */
  size?: number;
  className?: string;
}

/**
 * Logo "C" (prompt + pallino di stato). Fino a 20px usa la geometria
 * "small": tratti più spessi e pallino più grande, per restare leggibile.
 */
export default function Logo({ size = 24, className }: LogoProps) {
  const small = size <= 20;
  const stroke = small ? 3.4 : 2.6;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 32 32"
      fill="none"
      aria-hidden="true"
      className={className}
    >
      <rect x="1" y="1" width="30" height="30" rx="8" fill="#0A0E12" stroke="#24303D" />
      <path
        d="M8 11.5l5 5-5 5"
        stroke="#38BDF8"
        strokeWidth={stroke}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path d="M15.5 22h6" stroke="#38BDF8" strokeWidth={stroke} strokeLinecap="round" />
      <circle cx="23.5" cy="9" r={small ? 3.6 : 3} fill="#FB923C" />
    </svg>
  );
}
