import { useEffect, useRef, type ReactNode } from "react";

export interface MenuEntry {
  label?: string;
  icon?: ReactNode;
  danger?: boolean;
  disabled?: boolean;
  separator?: boolean;
  onClick?: () => void;
}

interface Props {
  x: number;
  y: number;
  entries: MenuEntry[];
  onClose: () => void;
}

/** Right-click menu, clamped to the viewport, dismissed like a native one. */
export function ContextMenu({ x, y, entries, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (el) {
      const rect = el.getBoundingClientRect();
      if (x + rect.width > window.innerWidth) el.style.left = `${x - rect.width}px`;
      if (y + rect.height > window.innerHeight) el.style.top = `${y - rect.height}px`;
    }
    const dismiss = () => onClose();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    // Capture phase so any click anywhere closes first.
    window.addEventListener("mousedown", dismiss, true);
    window.addEventListener("blur", dismiss);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", dismiss, true);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("keydown", onKey);
    };
  }, [x, y, onClose]);

  return (
    <div ref={ref} className="menu" style={{ left: x, top: y }} role="menu">
      {entries.map((entry, i) =>
        entry.separator ? (
          <div key={i} className="menu-sep" />
        ) : (
          <button
            key={i}
            role="menuitem"
            disabled={entry.disabled}
            data-danger={entry.danger ? "" : undefined}
            className="menu-item disabled:opacity-40"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={() => {
              onClose();
              entry.onClick?.();
            }}
          >
            {entry.icon && <span className="text-t2">{entry.icon}</span>}
            {entry.label}
          </button>
        ),
      )}
    </div>
  );
}
