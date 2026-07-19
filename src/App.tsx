import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import type { AppConfig, Candidate, Game, ToolStatus } from "./types";
import { Onboarding } from "./components/Onboarding";
import { Sidebar, type Tab } from "./components/Sidebar";
import { LibraryView } from "./components/LibraryView";
import { DiscoveriesView } from "./components/DiscoveriesView";
import { SetupView } from "./components/SetupView";

export default function App() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [games, setGames] = useState<Game[]>([]);
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [tools, setTools] = useState<ToolStatus[]>([]);
  const [tab, setTab] = useState<Tab>("library");
  const [runningGameIds, setRunningGameIds] = useState<Set<number>>(new Set());
  const [toast, setToast] = useState<string | null>(null);

  const notify = useCallback((message: string) => {
    setToast(message);
    window.setTimeout(() => setToast(null), 6000);
  }, []);

  const refresh = useCallback(async () => {
    const cfg = await api.getConfig();
    setConfig(cfg);
    setTools(await api.doctor());
    if (cfg.library_root) {
      const [g, c] = await Promise.all([api.listGames(), api.listCandidates()]);
      setGames(g);
      setCandidates(c);
    }
  }, []);

  useEffect(() => {
    refresh().catch((e) => notify(String(e)));
  }, [refresh, notify]);

  useEffect(() => {
    const unlisten = api.onGameExited((e) => {
      setRunningGameIds((prev) => {
        const next = new Set(prev);
        next.delete(e.game_id);
        return next;
      });
      if (e.error) {
        notify(`Launch failed: ${e.error}`);
      }
      refresh().catch((err) => notify(String(err)));
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => undefined);
    };
  }, [refresh, notify]);

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
        notify(String(e));
      }
    },
    [notify],
  );

  if (config === null) {
    return (
      <div className="flex h-full items-center justify-center text-zinc-500">
        Loading…
      </div>
    );
  }

  if (!config.library_root) {
    return (
      <Onboarding
        onChosen={async (path) => {
          try {
            setConfig(await api.setLibraryRoot(path));
            await refresh();
          } catch (e) {
            notify(String(e));
          }
        }}
      />
    );
  }

  return (
    <div className="flex h-full">
      <Sidebar
        tab={tab}
        onTab={setTab}
        gameCount={games.length}
        candidateCount={candidates.filter((c) => c.installed_game_id === null).length}
        missingRequiredTools={tools.some((t) => t.required && !t.found)}
      />
      <main className="min-w-0 flex-1 overflow-y-auto">
        {tab === "library" && (
          <LibraryView
            games={games}
            runningGameIds={runningGameIds}
            onLaunch={launch}
            onRemoved={() => refresh().catch((e) => notify(String(e)))}
            onError={notify}
          />
        )}
        {tab === "discoveries" && (
          <DiscoveriesView
            candidates={candidates}
            onChanged={() => refresh().catch((e) => notify(String(e)))}
            onError={notify}
            onInstalled={(game) => notify(`Installed ${game.title}`)}
          />
        )}
        {tab === "setup" && (
          <SetupView
            config={config}
            tools={tools}
            onLibraryChanged={() => refresh().catch((e) => notify(String(e)))}
            onError={notify}
          />
        )}
      </main>
      {toast && (
        <div className="fixed bottom-6 left-1/2 z-50 max-w-xl -translate-x-1/2 rounded-lg border border-ink-700 bg-ink-800 px-4 py-3 text-sm shadow-2xl">
          {toast}
        </div>
      )}
    </div>
  );
}
