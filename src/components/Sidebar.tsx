export type Tab = "library" | "discoveries" | "setup";

interface Props {
  tab: Tab;
  onTab: (tab: Tab) => void;
  gameCount: number;
  candidateCount: number;
  missingRequiredTools: boolean;
}

const ITEMS: { key: Tab; label: string; icon: string }[] = [
  { key: "library", label: "Library", icon: "▦" },
  { key: "discoveries", label: "Discoveries", icon: "◎" },
  { key: "setup", label: "Setup", icon: "⚙" },
];

export function Sidebar({ tab, onTab, gameCount, candidateCount, missingRequiredTools }: Props) {
  return (
    <aside className="flex w-56 shrink-0 flex-col border-r border-ink-800 bg-ink-900">
      <div className="flex items-center gap-2 px-5 py-5">
        <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-accent-600 text-sm font-bold text-white">
          ▶
        </div>
        <span className="text-lg font-semibold tracking-tight text-white">Pezo</span>
      </div>
      <nav className="flex flex-col gap-1 px-3">
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
              className={`flex items-center justify-between rounded-lg px-3 py-2 text-left text-sm transition-colors ${
                active
                  ? "bg-ink-700 text-white"
                  : "text-zinc-400 hover:bg-ink-800 hover:text-zinc-200"
              }`}
            >
              <span className="flex items-center gap-2.5">
                <span className="text-base leading-none">{item.icon}</span>
                {item.label}
                {item.key === "setup" && missingRequiredTools && (
                  <span className="h-2 w-2 rounded-full bg-amber-400" title="Missing tools" />
                )}
              </span>
              {badge > 0 && (
                <span className="rounded-full bg-ink-950 px-2 py-0.5 text-xs text-zinc-400">
                  {badge}
                </span>
              )}
            </button>
          );
        })}
      </nav>
      <div className="mt-auto px-5 py-4 text-xs text-zinc-600">
        GOG installers · abandonware · DOS
      </div>
    </aside>
  );
}
