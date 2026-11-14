import type React from "react";

export type IconName =
  | "book"
  | "grid"
  | "list"
  | "search"
  | "plus"
  | "settings"
  | "device"
  | "arrow-left"
  | "arrow-right"
  | "check"
  | "upload"
  | "download"
  | "edit"
  | "more"
  | "folder"
  | "link"
  | "mic"
  | "headphones"
  | "close"
  | "chevron-down"
  | "wand"
  | "leaf"
  | "external";

export interface IconProps
  extends Omit<React.SVGProps<SVGSVGElement>, "name" | "children"> {
  name: IconName;
  size?: number;
}

const DRAWINGS: Record<IconName, React.ReactNode> = {
  book: (
    <>
      <path d="M12 6.5C9 4.5 5.5 4.5 3 5.5v14c2.5-1 6-1 9 1 3-2 6.5-2 9-1v-14c-2.5-1-6-1-9 1Z" />
      <path d="M12 6.5v14M6 9h3M6 12h3M15 9h3M15 12h3" />
    </>
  ),
  grid: (
    <>
      <rect x="3" y="3" width="7" height="7" rx="2" />
      <rect x="14" y="3" width="7" height="7" rx="2" />
      <rect x="3" y="14" width="7" height="7" rx="2" />
      <rect x="14" y="14" width="7" height="7" rx="2" />
    </>
  ),
  list: (
    <>
      <path d="M9 5h12M9 12h12M9 19h12" />
      <circle cx="4" cy="5" r="1" />
      <circle cx="4" cy="12" r="1" />
      <circle cx="4" cy="19" r="1" />
    </>
  ),
  search: (
    <>
      <circle cx="10.5" cy="10.5" r="6.5" />
      <path d="m16 16 5 5" />
    </>
  ),
  plus: <path d="M12 5v14M5 12h14" />,
  settings: (
    <>
      <path d="m9.5 3-.7 2.2-2 .9-2.1-.5-2.5 4.3 1.5 1.7v2.2L2.2 15l2.5 4.3 2.1-.5 2 .9.7 2.3h5l.7-2.3 2-.9 2.1.5 2.5-4.3-1.5-1.7v-2.2l1.5-1.7-2.5-4.3-2.1.5-2-.9-.7-2.2Z" />
      <circle cx="12" cy="12" r="3.2" />
    </>
  ),
  device: (
    <>
      <rect x="4" y="3" width="16" height="18" rx="4" />
      <rect x="7" y="6" width="10" height="7" rx="1.5" />
      <path d="M8 17h3M9.5 15.5v3" />
      <circle cx="16" cy="17" r="1" />
    </>
  ),
  "arrow-left": <path d="m10 5-7 7 7 7M3 12h18" />,
  "arrow-right": <path d="m14 5 7 7-7 7M21 12H3" />,
  check: <path d="m5 12 4.5 4.5L19 7" />,
  upload: (
    <>
      <path d="M12 15V3m-5 5 5-5 5 5M4 15v4a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4" />
    </>
  ),
  download: (
    <>
      <path d="M12 3v12m-5-5 5 5 5-5M4 15v4a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4" />
    </>
  ),
  edit: (
    <>
      <path d="m15.5 4.5 4 4M4 20l4.5-1L20 7.5a2.8 2.8 0 0 0-4-4L4.5 15Z" />
      <path d="M14 20h7" />
    </>
  ),
  more: (
    <>
      <circle cx="5" cy="12" r="1.5" />
      <circle cx="12" cy="12" r="1.5" />
      <circle cx="19" cy="12" r="1.5" />
    </>
  ),
  folder: (
    <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
  ),
  link: (
    <>
      <path d="m9 15 6-6M8 16l-1 1a3.5 3.5 0 0 1-5-5l5-5a3.5 3.5 0 0 1 5 0m0 10a3.5 3.5 0 0 0 5 0l5-5a3.5 3.5 0 0 0-5-5l-1 1" />
    </>
  ),
  mic: (
    <>
      <rect x="8" y="2" width="8" height="13" rx="4" />
      <path d="M5 11a7 7 0 0 0 14 0M12 18v4M8 22h8" />
    </>
  ),
  headphones: (
    <>
      <path d="M4 13v-1a8 8 0 0 1 16 0v1" />
      <rect x="3" y="12" width="5" height="8" rx="2" />
      <rect x="16" y="12" width="5" height="8" rx="2" />
    </>
  ),
  close: <path d="m6 6 12 12M6 18 18 6" />,
  "chevron-down": <path d="m6 9 6 6 6-6" />,
  wand: (
    <>
      <path d="m4 20 13-13 3 3L7 23ZM13 11l3 3M6 3v4M4 5h4M18 1v4M16 3h4M21 16v4M19 18h4" />
    </>
  ),
  leaf: (
    <>
      <path d="M20 3C9 2 3 7 4 14c1 7 10 8 14 1 2-3 2-7 2-12Z" />
      <path d="M3 21 15 9M8 16v-5M8 16h5" />
    </>
  ),
  external: (
    <>
      <path d="M14 3h7v7M21 3 10 14M10 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-5" />
    </>
  ),
};

/** Decorative inline SVG. Give the surrounding control a visible label or
 * aria-label; an icon never replaces the accessible name of an action. */
export function Icon({
  name,
  size = 20,
  className,
  ...rest
}: IconProps): React.JSX.Element {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      className={["ds-icon", className].filter(Boolean).join(" ")}
      {...rest}
    >
      {DRAWINGS[name]}
    </svg>
  );
}
