// 16px stroke icons, GNOME-symbolic style. One component per glyph keeps JSX terse.
import type { SVGProps } from "react";

function base(props: SVGProps<SVGSVGElement>) {
  return {
    width: 16,
    height: 16,
    viewBox: "0 0 16 16",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.5,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    ...props,
  };
}

export const IconLibrary = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <rect x="2" y="2" width="5" height="5" rx="1" />
    <rect x="9" y="2" width="5" height="5" rx="1" />
    <rect x="2" y="9" width="5" height="5" rx="1" />
    <rect x="9" y="9" width="5" height="5" rx="1" />
  </svg>
);

export const IconCompass = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <circle cx="8" cy="8" r="6" />
    <path d="M10.5 5.5 9 9l-3.5 1.5L7 7z" />
  </svg>
);

export const IconGear = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <circle cx="8" cy="8" r="2" />
    <path d="M8 1.8v2M8 12.2v2M14.2 8h-2M3.8 8h-2M12.4 3.6l-1.4 1.4M5 11l-1.4 1.4M12.4 12.4 11 11M5 5 3.6 3.6" />
  </svg>
);

export const IconPlay = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)} fill="currentColor" stroke="none">
    <path d="M4.5 2.8a.6.6 0 0 1 .9-.5l7.2 5.2a.6.6 0 0 1 0 1L5.4 13.7a.6.6 0 0 1-.9-.5z" />
  </svg>
);

export const IconSearch = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <circle cx="7" cy="7" r="4.5" />
    <path d="m10.5 10.5 3 3" />
  </svg>
);

export const IconGrid = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <rect x="2.5" y="2.5" width="4.5" height="4.5" />
    <rect x="9" y="2.5" width="4.5" height="4.5" />
    <rect x="2.5" y="9" width="4.5" height="4.5" />
    <rect x="9" y="9" width="4.5" height="4.5" />
  </svg>
);

export const IconList = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M5 4h9M5 8h9M5 12h9" />
    <path d="M2 4h.01M2 8h.01M2 12h.01" strokeWidth="2" />
  </svg>
);

export const IconRefresh = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9" />
    <path d="M13.5 2.5v2.7h-2.7" />
  </svg>
);

export const IconFolder = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.4 1.5h5A1.5 1.5 0 0 1 14 6v5.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5z" />
  </svg>
);

export const IconWine = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M5 2h6l-.5 4a2.5 2.5 0 0 1-5 0z" />
    <path d="M8 8.5V13M5.5 13h5" />
  </svg>
);

export const IconImage = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <rect x="2" y="3" width="12" height="10" rx="1.5" />
    <circle cx="5.5" cy="6.5" r="1" fill="currentColor" stroke="none" />
    <path d="m2.5 11.5 3-3 2.5 2.5 2.5-2.5 3 3" />
  </svg>
);

export const IconTrash = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M3 4.5h10M6.5 2.5h3M5 4.5l.5 8a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1l.5-8" />
  </svg>
);

export const IconCopy = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <rect x="5.5" y="5.5" width="8" height="8" rx="1" />
    <path d="M10.5 5.5v-2a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2" />
  </svg>
);

export const IconBack = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M10 3 5 8l5 5" />
  </svg>
);

export const IconMinimize = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M4 8.5h8" />
  </svg>
);

export const IconMaximize = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <rect x="4" y="4" width="8" height="8" rx="1" />
  </svg>
);

export const IconClose = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="m4.5 4.5 7 7M11.5 4.5l-7 7" />
  </svg>
);

export const IconSidebar = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <rect x="2" y="3" width="12" height="10" rx="1.5" />
    <path d="M6 3v10" />
  </svg>
);

export const IconWarning = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M8 2.5 14 13H2z" />
    <path d="M8 6.5V9.5M8 11.5h.01" strokeWidth="1.8" />
  </svg>
);
