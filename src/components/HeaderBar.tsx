import { useEffect, useState } from "react";
import { api } from "../api";
import type { DesktopLayout } from "../types";
import { IconClose, IconMaximize, IconMinimize, IconSidebar } from "./Icons";
import { mockEnabled } from "../mock";

async function windowControl(action: "minimize" | "maximize" | "close") {
  if (mockEnabled) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const win = getCurrentWindow();
  if (action === "minimize") await win.minimize();
  else if (action === "maximize") await win.toggleMaximize();
  else await win.close();
}

function WindowButtons({ buttons }: { buttons: string[] }) {
  if (buttons.length === 0) return null;
  return (
    <div className="flex items-center gap-1 px-1">
      {buttons.map((button) =>
        button === "minimize" ? (
          <button
            key={button}
            className="win-btn"
            onClick={() => windowControl("minimize")}
            aria-label="Minimize"
          >
            <IconMinimize width={13} height={13} />
          </button>
        ) : button === "maximize" ? (
          <button
            key={button}
            className="win-btn"
            onClick={() => windowControl("maximize")}
            aria-label="Maximize"
          >
            <IconMaximize width={12} height={12} />
          </button>
        ) : (
          <button
            key={button}
            className="win-btn hover:bg-danger hover:text-white"
            onClick={() => windowControl("close")}
            aria-label="Close"
          >
            <IconClose width={13} height={13} />
          </button>
        ),
      )}
    </div>
  );
}

interface Props {
  onToggleSidebar?: () => void;
  slotRef?: (el: HTMLDivElement | null) => void;
}

/**
 * Client-side-decorated headerbar, GNOME style: sidebar toggle and app
 * identity on the left, the active view's controls in the middle (portaled in
 * by the view), window buttons arranged to match the desktop's own layout.
 */
export function HeaderBar({ onToggleSidebar, slotRef }: Props) {
  const [layout, setLayout] = useState<DesktopLayout>({ left: [], right: ["close"] });

  useEffect(() => {
    api
      .desktopLayout()
      .then(setLayout)
      .catch(() => setLayout({ left: [], right: ["minimize", "maximize", "close"] }));
  }, []);

  return (
    <div className="headerbar" data-tauri-drag-region>
      <WindowButtons buttons={layout.left} />
      {onToggleSidebar && (
        <button
          className="win-btn rounded-md"
          onClick={onToggleSidebar}
          title="Toggle sidebar (F9)"
          aria-label="Toggle sidebar"
        >
          <IconSidebar width={14} height={14} />
        </button>
      )}
      <span className="wordmark px-1.5 text-lg" data-tauri-drag-region>
        pezo
      </span>
      <div
        ref={slotRef}
        className="flex min-w-0 flex-1 items-center justify-end gap-2"
        data-tauri-drag-region
      />
      <WindowButtons buttons={layout.right} />
    </div>
  );
}
