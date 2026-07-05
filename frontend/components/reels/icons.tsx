// Inline SVG icons for the reels feed, matching the Figma "Glass · Reels" design.
// Self-contained (no external/expiring asset URLs); all use currentColor so the
// caller controls the colour. 24×24 viewBox, line icons at 2px stroke.

import type { ReactNode, SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function Svg({ size = 24, children, ...rest }: IconProps & { children: ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...rest}
    >
      {children}
    </svg>
  );
}

export function HeartIcon({ filled, ...p }: IconProps & { filled?: boolean }) {
  return (
    <Svg {...p}>
      <path
        d="M12 20.5C12 20.5 3.5 15.5 3.5 9.5C3.5 6.9 5.5 5 8 5C9.6 5 11 5.9 12 7.2C13 5.9 14.4 5 16 5C18.5 5 20.5 6.9 20.5 9.5C20.5 15.5 12 20.5 12 20.5Z"
        fill={filled ? "currentColor" : "none"}
      />
    </Svg>
  );
}

export function CommentIcon(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M21 11.5C21 16 16.97 19.5 12 19.5C10.7 19.5 9.46 19.27 8.33 18.85L3.5 20.5L5.1 15.9C4.09 14.63 3.5 13.12 3.5 11.5C3.5 7 7.53 3.5 12 3.5C16.97 3.5 21 7 21 11.5Z" />
    </Svg>
  );
}

export function CoinIcon(p: IconProps) {
  return (
    <Svg {...p}>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M14.5 9.2C13.9 8.5 13 8.1 12 8.1C10 8.1 8.8 9.9 8.8 12C8.8 14.1 10 15.9 12 15.9C13 15.9 13.9 15.5 14.5 14.8" />
      <path d="M7.8 11H12.6M7.8 13H12.2" />
    </Svg>
  );
}

export function ShareIcon(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M12 15.5V4M8 7.5L12 3.5L16 7.5" />
      <path d="M5 14.5V18.5C5 19.6 5.9 20.5 7 20.5H17C18.1 20.5 19 19.6 19 18.5V14.5" />
    </Svg>
  );
}

export function BookmarkIcon({ filled, ...p }: IconProps & { filled?: boolean }) {
  return (
    <Svg {...p}>
      <path d="M6.5 4.5H17.5V20L12 16.2L6.5 20V4.5Z" fill={filled ? "currentColor" : "none"} />
    </Svg>
  );
}

export function MusicIcon(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M9 17.5V5.5L19 3.5V15.5" />
      <circle cx="6.5" cy="17.5" r="2.5" />
      <circle cx="16.5" cy="15.5" r="2.5" />
    </Svg>
  );
}

export function VerifiedIcon({ size = 16, ...p }: IconProps) {
  // Filled badge with a check — coloured by the caller (blue accent).
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true" {...p}>
      <path
        d="M12 2L14.09 3.5L16.67 3.24L17.76 5.6L20.12 6.69L19.86 9.27L21.36 11.36L19.86 13.45L20.12 16.03L17.76 17.12L16.67 19.48L14.09 19.22L12 20.72L9.91 19.22L7.33 19.48L6.24 17.12L3.88 16.03L4.14 13.45L2.64 11.36L4.14 9.27L3.88 6.69L6.24 5.6L7.33 3.24L9.91 3.5L12 2Z"
        fill="currentColor"
      />
      <path d="M8.5 11.5L11 14L15.5 9" stroke="#fff" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

export function HomeIcon({ filled, ...p }: IconProps & { filled?: boolean }) {
  return (
    <Svg {...p}>
      <path d="M3.5 11L12 4L20.5 11" />
      <path d="M5.5 9.8V19C5.5 19.6 6 20 6.5 20H17.5C18 20 18.5 19.6 18.5 19V9.8" fill={filled ? "currentColor" : "none"} />
    </Svg>
  );
}

export function SearchIcon(p: IconProps) {
  return (
    <Svg {...p}>
      <circle cx="11" cy="11" r="7" />
      <path d="M20 20L16.5 16.5" />
    </Svg>
  );
}

export function CreateIcon(p: IconProps) {
  return (
    <Svg {...p}>
      <rect x="3.5" y="3.5" width="17" height="17" rx="4" />
      <path d="M12 8.5V15.5M8.5 12H15.5" />
    </Svg>
  );
}

export function MessageIcon(p: IconProps) {
  return (
    <Svg {...p}>
      <rect x="3.5" y="5" width="17" height="14" rx="3" />
      <path d="M4.5 7L12 12.5L19.5 7" />
    </Svg>
  );
}

export function ProfileIcon({ filled, ...p }: IconProps & { filled?: boolean }) {
  return (
    <Svg {...p}>
      <circle cx="12" cy="8" r="3.8" fill={filled ? "currentColor" : "none"} />
      <path d="M4.5 20C4.5 16.4 7.9 14 12 14C16.1 14 19.5 16.4 19.5 20" fill={filled ? "currentColor" : "none"} />
    </Svg>
  );
}

export function ChevronUpIcon(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M6 15L12 9L18 15" />
    </Svg>
  );
}

export function ChevronDownIcon(p: IconProps) {
  return (
    <Svg {...p}>
      <path d="M6 9L12 15L18 9" />
    </Svg>
  );
}

export function LockIcon({ size = 40, ...p }: IconProps) {
  return (
    <Svg size={size} strokeWidth={1.8} {...p}>
      <rect x="4.5" y="10.5" width="15" height="10" rx="2.5" />
      <path d="M7.5 10.5V7.5C7.5 5 9.5 3 12 3C14.5 3 16.5 5 16.5 7.5V10.5" />
      <circle cx="12" cy="15.5" r="1.4" fill="currentColor" />
    </Svg>
  );
}

export function ImageIcon({ size = 72, ...p }: IconProps) {
  return (
    <Svg size={size} strokeWidth={1.5} {...p}>
      <rect x="3" y="5" width="18" height="14" rx="2.5" />
      <circle cx="8.5" cy="10" r="1.8" />
      <path d="M4 17.5L9 12.5L13 16.5L16 13.5L20 17.5" />
    </Svg>
  );
}
