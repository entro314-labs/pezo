import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import { mockEnabled } from "./mock";
import type {
  AppConfig,
  Candidate,
  FsInfo,
  Game,
  QueueTask,
  ScanProgress,
  ToolStatus,
} from "./types";
import { Onboarding } from "./components/Onboarding";
import { HeaderBar } from "./components/HeaderBar";
import { StatusBar } from "./components/StatusBar";
import { Sidebar, type Tab } from "./components/Sidebar";
import { LibraryView } from "./components/LibraryView";
import { GameDetail } from "./components/GameDetail";
import { DiscoveriesView } from "./components/DiscoveriesView";
import { SetupView } from "./components/SetupView";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { QueuePanel } from "./components/QueuePanel";
import { Toasts, type Toast, type ToastVariant } from "./components/Toasts";

export default function App() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [games, setGames] = useState<Game[]>([]);
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [covers, setCovers] = useState<Record<number, string>>({});
  const [tools, setTools] = useState<ToolStatus[]>([]);
  const [fsInfo, setFsInfo] = useState<FsInfo | null>(null);
  const [tab, setTab] = useState<Tab>("library");
  const [openGameId, setOpenGameId] = useState<number | null>(null);
  const [runningGameIds, setRunningGameIds] = useState<Set<number>>(new Set());
  const [scanProgress, setScanProgress] = useState<ScanProgress | null>(null);
  const [queue, setQueue] = useState<QueueTask[]>([]);
  const [queueOpen, setQueueOpen] = useState(false);
  const prevQueue = useRef<Map<number, QueueTask["status"]>>(new Map());
  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastId = useRef(0);
  const [sidebarOpen, setSidebarOpen] = useState(
    () => localStorage.getItem("pezo-sidebar") !== "0",
  );
  const [toolbarSlot, setToolbarSlot] = useState<HTMLDivElement | null>(null);

  const toggleSidebar = useCallback(() => {
    setSidebarOpen((open) => {
      localStorage.setItem("pezo-sidebar", open ? "0" : "1");
      return !open;
    });
  }, []);

  /** DBus desktop notification for long operations finishing while unfocused. */
  const notifyDesktop = useCallback(async (title: string, body: string) => {
    if (mockEnabled || document.hasFocus()) return;
    try {
      const n = await import("@tauri-apps/plugin-notification");
      let granted = await n.isPermissionGranted();
      if (!granted) granted = (await n.requestPermission()) === "granted";
      if (granted) n.sendNotification({ title, body });
    } catch {
      // Notifications are a nicety, never an error.
    }
  }, []);

  const pushToast = useCallback((message: string, variant: ToastVariant) => {
    const id = ++toastId.current;
    setToasts((prev) => [...prev.slice(-3), { id, message, variant }]);
    window.setTimeout(
      () => setToasts((prev) => prev.filter((t) => t.id !== id)),
      variant === "error" ? 10000 : 5000,
    );
  }, []);

  const notify = useCallback((m: string) => pushToast(m, "info"), [pushToast]);
  const notifyError = useCallback((m: string) => pushToast(m, "error"), [pushToast]);

  const refresh = useCallback(async () => {
    const cfg = await api.getConfig();
    setConfig(cfg);
    setTools(await api.doctor());
    if (cfg.library_root) {
      const [g, c, fs] = await Promise.all([
        api.listGames(),
        api.listCandidates(),
        api.libraryFsInfo().catch(() => null),
      ]);
      setGames(g);
      setCandidates(c);
      setFsInfo(fs);

      const entries = await Promise.all(
        g.map(async (game) => [game.id, await api.getCover(game.id).catch(() => null)] as const),
      );
      const next: Record<number, string> = {};
      for (const [id, url] of entries) {
        if (url) next[id] = url;
      }
      setCovers(next);
    }
  }, []);

  useEffect(() => {
    refresh().catch((e) => notifyError(String(e)));
  }, [refresh, notifyError]);

  useEffect(() => {
    const unlistenExit = api.onGameExited((e) => {
      setRunningGameIds((prev) => {
        const next = new Set(prev);
        next.delete(e.game_id);
        return next;
      });
      if (e.error) {
        notifyError(`Launch failed: ${e.error}`);
      }
      refresh().catch((err) => notifyError(String(err)));
    });
    const unlistenScan = api.onScanProgress((p) => {
      setScanProgress(p.done >= p.total && p.total > 0 ? null : p);
    });
    return () => {
      unlistenExit.then((fn) => fn()).catch(() => undefined);
      unlistenScan.then((fn) => fn()).catch(() => undefined);
    };
  }, [refresh, notifyError]);

  // Mirror the backend install queue; completions surface as toasts and
  // desktop notifications, and finished installs refresh the library.
  useEffect(() => {
    api.queueState().then(setQueue).catch(() => undefined);
    const unlisten = api.onQueueChanged((tasks) => {
      setQueue(tasks);
      for (const task of tasks) {
        const prev = prevQueue.current.get(task.id);
        if (prev !== task.status) {
          if (task.status === "done") {
            pushToast(`Installed ${task.title}`, "success");
            void notifyDesktop("Pezo", `Installed ${task.title}`);
            refresh().catch(() => undefined);
          } else if (task.status === "failed") {
            pushToast(`${task.title}: ${task.error ?? "install failed"}`, "error");
            void notifyDesktop("Pezo — install failed", task.title);
          }
        }
      }
      prevQueue.current = new Map(tasks.map((t) => [t.id, t.status]));
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => undefined);
    };
  }, [pushToast, notifyDesktop, refresh]);

  // Desktop keyboard model: Ctrl+F search, Esc back, F9 sidebar, F5 rescan.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "f") {
        const search = document.getElementById("view-search") as HTMLInputElement | null;
        if (search) {
          e.preventDefault();
          search.focus();
          search.select();
        }
      } else if (e.key === "Escape") {
        setOpenGameId(null);
      } else if (e.key === "F9") {
        e.preventDefault();
        toggleSidebar();
      } else if (e.key === "F5") {
        e.preventDefault();
        window.dispatchEvent(new CustomEvent("pezo:scan"));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggleSidebar]);

  const launch = useCallback(
    async (game: Game) => {
      try {
        setRunningGameIds((prev) => new Set(prev).add(game.id));
        await api.launch(game.id);
      } catch (e) {
        setRunningGameIds((prev) => {
          const next = new Set(prev);
          next.delete(game.id);
          return next;
        });
        notifyError(String(e));
      }
    },
    [notifyError],
  );

  if (config === null) {
    return <div className="flex h-full items-center justify-center text-t3">Loading…</div>;
  }

  if (!config.library_root) {
    return (
      <div className="flex h-full flex-col">
        <HeaderBar />
        <Onboarding
          onChosen={async (path) => {
            try {
              setConfig(await api.setLibraryRoot(path));
              await refresh();
            } catch (e) {
              notifyError(String(e));
            }
          }}
        />
      </div>
    );
  }

  const refreshOrToast = () => refresh().catch((e) => notifyError(String(e)));
  const openGame = openGameId !== null ? (games.find((g) => g.id === openGameId) ?? null) : null;

  return (
    <div className="flex h-full flex-col">
      <HeaderBar onToggleSidebar={toggleSidebar} slotRef={setToolbarSlot} />
      <div className="flex min-h-0 flex-1">
        {sidebarOpen && (
          <Sidebar
            tab={tab}
            onTab={(next) => {
              setTab(next);
              setOpenGameId(null);
            }}
            gameCount={games.length}
            candidateCount={candidates.filter((c) => c.installed_game_id === null).length}
            missingRequiredTools={tools.some((t) => t.required && !t.found)}
          />
        )}
        <main className="flex min-w-0 flex-1 flex-col">
          <ErrorBoundary>
            {tab === "library" &&
              (openGame ? (
                <GameDetail
                  game={openGame}
                  cover={covers[openGame.id] ?? null}
                  running={runningGameIds.has(openGame.id)}
                  defaultWine={config?.default_wine ?? null}
                  onLaunch={launch}
                  onBack={() => setOpenGameId(null)}
                  onChanged={refreshOrToast}
                  onError={notifyError}
                  onNotice={notify}
                />
              ) : (
                <LibraryView
                  games={games}
                  covers={covers}
                  runningGameIds={runningGameIds}
                  toolbarSlot={toolbarSlot}
                  onLaunch={launch}
                  onOpen={(game) => setOpenGameId(game.id)}
                  onChanged={refreshOrToast}
                  onError={notifyError}
                  onNotice={notify}
                />
              ))}
            {tab === "discoveries" && (
              <DiscoveriesView
                candidates={candidates}
                config={config}
                queue={queue}
                toolbarSlot={toolbarSlot}
                onChanged={refreshOrToast}
                onError={notifyError}
                onNotice={notify}
                onScanDone={(summary) => void notifyDesktop("Pezo — scan finished", summary)}
              />
            )}
            {tab === "settings" && (
              <SetupView
                config={config}
                tools={tools}
                fsInfo={fsInfo}
                onLibraryChanged={refreshOrToast}
                onError={notifyError}
                onNotice={notify}
              />
            )}
          </ErrorBoundary>
        </main>
      </div>
      <StatusBar
        gameCount={games.length}
        discoveryCount={candidates.filter((c) => c.installed_game_id === null).length}
        runningCount={runningGameIds.size}
        queue={queue}
        queueOpen={queueOpen}
        onToggleQueue={() => setQueueOpen((open) => !open)}
        scanProgress={scanProgress}
        fsInfo={fsInfo}
        libraryRoot={config.library_root}
      />
      {queueOpen && (
        <QueuePanel
          tasks={queue}
          onCancel={(taskId) => api.queueCancel(taskId).catch((e) => notifyError(String(e)))}
          onClear={() => api.queueClearFinished().catch((e) => notifyError(String(e)))}
          onClose={() => setQueueOpen(false)}
        />
      )}
      <Toasts
        toasts={toasts}
        onDismiss={(id) => setToasts((prev) => prev.filter((t) => t.id !== id))}
      />
    </div>
  );
}
