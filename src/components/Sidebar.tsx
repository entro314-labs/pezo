import type { ReactNode } from "react";
import { IconCompass, IconGear, IconLibrary } from "./Icons";

export type Tab = "library" | "discoveries" | "settings";

interface Props {
  tab: Tab;
  onTab: (tab: Tab) => void;
  gameCount: number;
  candidateCount: number;
  missingRequiredTools: boolean;
}

const ITEMS: { key: Tab; label: string; icon: ReactNode }[] = [
  { key: "library", label: "Library", icon: <IconLibrary /> },
  { key: "discoveries", label: "Discoveries", icon: <IconCompass /> },
  { key: "settings", label: "Settings", icon: <IconGear /> },
];

export function Sidebar({ tab, onTab, gameCount, candidateCount, missingRequiredTools }: Props) {
  return (
    <aside className="flex w-44 shrink-0 flex-col border-r border-edge bg-surface pt-2">
      <nav className="flex flex-col gap-0.5 px-2">
        {ITEMS.map((item) => {
          const active = tab === item.key;
          const badge =
            item.key === "library"
              ? gameCount
              : item.key === "discoveries"
                ? candidateCount
                : 0;
          return (
            <button
              key={item.key}
              onClick={() => onTab(item.key)}
              className={`relative flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-[13px] transition-colors ${
                active ? "bg-accent-soft font-medium text-t1" : "text-t2 hover:bg-raised hover:text-t1"
              }`}
            >
              {active && (
                <span className="absolute top-1/2 left-0 h-4 w-0.5 -translate-y-1/2 rounded-full bg-accent" />
              )}
              <span className={active ? "text-accent" : "text-t3"}>{item.icon}</span>
              <span className="flex-1">{item.label}</span>
              {item.key === "settings" && missingRequiredTools && (
                <span className="h-1.5 w-1.5 rounded-full bg-warn" title="Missing tools" />
              )}
              {badge > 0 && <span className="text-[11px] text-t3">{badge}</span>}
            </button>
          );
        })}
      </nav>
      <div className="mt-auto px-4 py-3">
        <p className="eyebrow">GOG · DOS · Abandonware</p>
      </div>
    </aside>
  );
}
