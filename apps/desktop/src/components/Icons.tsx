import type { ReactNode, SVGProps } from "react";

type IconName =
  | "arrow"
  | "chevron-down"
  | "chevron-up"
  | "close"
  | "cloud"
  | "document"
  | "eye"
  | "eye-off"
  | "folder"
  | "fullscreen"
  | "grid"
  | "layers"
  | "menu"
  | "more"
  | "pause"
  | "pin"
  | "plus"
  | "question"
  | "save"
  | "search"
  | "settings"
  | "triangle"
  | "x";

export function Icon({ name, size = 18, strokeWidth = 1.7, ...props }: SVGProps<SVGSVGElement> & { name: IconName; size?: number; strokeWidth?: number }) {
  const common = {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    "aria-hidden": true,
    focusable: false,
    ...props,
  };
  const paths: Record<IconName, ReactNode> = {
    arrow: <path d="m5 12 14-8-4 8 4 8-14-8Z" fill="currentColor" stroke="none" />,
    "chevron-down": <path d="m6 9 6 6 6-6" />,
    "chevron-up": <path d="m6 15 6-6 6 6" />,
    close: <path d="m6 6 12 12M18 6 6 18" />,
    cloud: <path d="M7.2 18h10.1a3.7 3.7 0 0 0 .6-7.35A5.9 5.9 0 0 0 6.55 9.2 4.45 4.45 0 0 0 7.2 18Z" />,
    document: <path d="M7 3.8h7l4 4V20H7V3.8Zm7 0v4h4" />,
    eye: <><path d="M2.7 12s3.3-5.2 9.3-5.2 9.3 5.2 9.3 5.2-3.3 5.2-9.3 5.2S2.7 12 2.7 12Z" /><circle cx="12" cy="12" r="2.3" /></>,
    "eye-off": <><path d="m3 3 18 18M10.6 6.95A10.7 10.7 0 0 1 12 6.8c6 0 9.3 5.2 9.3 5.2a17 17 0 0 1-3.1 3.2M6.3 6.95C3.9 8.6 2.7 12 2.7 12s3.3 5.2 9.3 5.2c.8 0 1.6-.1 2.3-.3" /></>,
    folder: <><path d="M3.5 6.8h6l1.7 2h9.3v8.8a1.9 1.9 0 0 1-1.9 1.9H5.4a1.9 1.9 0 0 1-1.9-1.9V6.8Z" /><path d="M3.5 9h17" /></>,
    fullscreen: <><path d="M8.5 3.5H3.5v5M15.5 3.5h5v5M8.5 20.5H3.5v-5M15.5 20.5h5v-5" /></>,
    grid: <><path d="M3.5 3.5h7v7h-7zM13.5 3.5h7v7h-7zM3.5 13.5h7v7h-7zM13.5 13.5h7v7h-7z" /></>,
    layers: <><path d="m12 3 9 4.6-9 4.6-9-4.6L12 3Z" /><path d="m4.3 12.2-1.3.7 9 4.6 9-4.6-1.3-.7M4.3 16.2l-1.3.7 9 4.6 9-4.6-1.3-.7" /></>,
    menu: <><path d="M4 6h16M4 12h16M4 18h16" /></>,
    more: <><circle cx="5" cy="12" r=".8" fill="currentColor" /><circle cx="12" cy="12" r=".8" fill="currentColor" /><circle cx="19" cy="12" r=".8" fill="currentColor" /></>,
    pause: <><path d="M8 5v14M16 5v14" /></>,
    pin: <><path d="M8 3.8h8l-.8 5 2.8 3v1.5H6v-1.5l2.8-3-.8-5ZM12 13.3V21" /></>,
    plus: <><path d="M12 5v14M5 12h14" /></>,
    question: <><circle cx="12" cy="12" r="9" /><path d="M9.7 9a2.4 2.4 0 1 1 3.8 1.9c-.9.6-1.5 1-1.5 2.2M12 16.8h.01" /></>,
    save: <><path d="M4 4h13l3 3v13H4V4Z" /><path d="M8 4v5h7V4M8 20v-6h8v6" /></>,
    search: <><circle cx="10.8" cy="10.8" r="6.3" /><path d="m16 16 4.5 4.5" /></>,
    settings: <><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-1.6 1.6-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.5v.2H11v-.2a1.7 1.7 0 0 0-1-1.5 1.7 1.7 0 0 0-1.9.3l-.1.1-1.6-1.6.1-.1a1.7 1.7 0 0 0 .3-1.9 1.7 1.7 0 0 0-1.5-1H5v-2.3h.2a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.9l-.1-.1 1.6-1.6.1.1a1.7 1.7 0 0 0 1.9.3 1.7 1.7 0 0 0 1-1.5V5h2.3v.2a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1 1.6 1.6-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.5 1h.2v2.3h-.2a1.7 1.7 0 0 0-1.4 1Z" /></>,
    triangle: <path d="m12 3 9 17H3L12 3Z" />,
    x: <path d="m7 7 10 10M17 7 7 17" />,
  };
  return <svg {...common}>{paths[name]}</svg>;
}

export function LogoMark() {
  return <span className="logo-mark" aria-hidden="true"><span /><span /><span /></span>;
}
