import { useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, playtime } from "../api";
import type { Game } from "../types";
import { RUNNER_LABELS } from "../types";
import { ContextMenu, type MenuEntry } from "./ContextMenu";
import { IconFolder, IconGrid, IconImage, IconList, IconPlay, IconSearch, IconWine } from "./Icons";

interface Props {
  games: Game[];
  covers: Record<number, string>;
  runningGameIds: Set<number>;
  toolbarSlot: HTMLDivElement | null;
  onLaunch: (game: Game) => void;
  onOpen: (game: Game) => void;
  onChanged: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
}

type ViewMode = "grid" | "list";
type SortBy = "title" | "recent" | "playtime";

function tileGradient(slug: string): string {
  let hash = 0;
  for (const c of slug) {
    hash = (hash * 31 + c.charCodeAt(0)) | 0;
  }
  const hue = ((hash % 360) + 360) % 360;
  return `linear-gradient(140deg, hsl(${hue} 45% 38%), hsl(${(hue + 40) % 360} 55% 22%))`;
}

function initials(title: string): string {
  return title
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase() ?? "")
    .join("");
}

interface MenuState {
  x: number;
  y: number;
  game: Game;
}

export function LibraryView({
  games,
  covers,
  runningGameIds,
  toolbarSlot,
  onLaunch,
  onOpen,
  onChanged,
  onError,
  onNotice,
}: Props) {
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<ViewMode>("grid");
  const [sortBy, setSortBy] = useState<SortBy>("title");
  const [fetchingCovers, setFetchingCovers] = useState(false);
  const [menu, setMenu] = useState<MenuState | null>(null);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = q ? games.filter((g) => g.title.toLowerCase().includes(q)) : [...games];
    return filtered.sort((a, b) => {
      switch (sortBy) {
        case "playtime":
          return b.play_seconds - a.play_seconds;
        case "recent":
          return (b.last_played_at ?? "").localeCompare(a.last_played_at ?? "");
        default:
          return a.title.localeCompare(b.title, undefined, { sensitivity: "base" });
      }
    });
  }, [games, query, sortBy]);

  const fetchCovers = async () => {
    setFetchingCovers(true);
    try {
      const fetched = await api.fetchMissingCovers();
      onNotice(fetched > 0 ? `Fetched ${fetched} cover(s)` : "No new covers found");
      onChanged();
    } catch (e) {
      onError(String(e));
    } finally {
      setFetchingCovers(false);
    }
  };

  const gameMenu = (game: Game): MenuEntry[] => {
    const running = runningGameIds.has(game.id);
    return [
      {
        label: running ? "Running…" : "Play",
        icon: <IconPlay width={14} height={14} />,
        disabled: running,
        onClick: () => onLaunch(game),
      },
      { label: "Details", onClick: () => onOpen(game) },
      { separator: true },
      {
        label: "Open install folder",
        icon: <IconFolder width={14} height={14} />,
        onClick: () => revealItemInDir(game.install_path).catch((e) => onError(String(e))),
      },
      ...(game.prefix_path
        ? [
            {
              label: "Open wine prefix",
              icon: <IconFolder width={14} height={14} />,
              onClick: () =>
                revealItemInDir(game.prefix_path!).catch((e) => onError(String(e))),
            },
          ]
        : []),
      ...(game.runner === "proton"
        ? [
            {
              label: "Winetricks",
              icon: <IconWine width={14} height={14} />,
              onClick: () =>
                api
                  .runWinetricks(game.id)
                  .then(() => onNotice("Winetricks opening…"))
                  .catch((e) => onError(String(e))),
            },
          ]
        : []),
    ];
  };

  const openMenu = (e: React.MouseEvent, game: Game) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, game });
  };

  const toolbar = (
    <>
      <div className="relative">
        <IconSearch
          width={13}
          height={13}
          className="pointer-events-none absolute top-1/2 left-2 -translate-y-1/2 text-t3"
        />
        <input
          id="view-search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search"
          className="input w-48 pl-7"
        />
      </div>
      <select
        value={sortBy}
        onChange={(e) => setSortBy(e.target.value as SortBy)}
        className="input text-t2"
      >
        <option value="title">Title</option>
        <option value="recent">Recently played</option>
        <option value="playtime">Playtime</option>
      </select>
      <button
        onClick={fetchCovers}
        disabled={fetchingCovers}
        className="btn-ghost h-7 w-7 p-0"
        title="Fetch missing covers"
      >
        <IconImage />
      </button>
      <div className="flex overflow-hidden rounded-md border border-edge">
        <button
          onClick={() => setMode("grid")}
          className={`flex h-6.5 w-7 items-center justify-center ${
            mode === "grid" ? "bg-accent-soft text-t1" : "text-t3 hover:text-t1"
          }`}
          title="Grid"
        >
          <IconGrid width={13} height={13} />
        </button>
        <button
          onClick={() => setMode("list")}
          className={`flex h-6.5 w-7 items-center justify-center ${
            mode === "list" ? "bg-accent-soft text-t1" : "text-t3 hover:text-t1"
          }`}
          title="List"
        >
          <IconList width={13} height={13} />
        </button>
      </div>
    </>
  );

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col">
      {toolbarSlot && createPortal(toolbar, toolbarSlot)}

      <div className="min-h-0 flex-1 overflow-y-auto">
        {games.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-1.5 text-center">
            <p className="text-sm text-t2">No games installed yet</p>
            <p className="text-[12px] text-t3">Scan Discoveries and install something.</p>
          </div>
        ) : visible.length === 0 ? (
          <p className="p-8 text-center text-[12px] text-t3">No games match “{query}”.</p>
        ) : mode === "grid" ? (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(132px,1fr))] gap-3.5 p-4">
            {visible.map((game) => {
              const running = runningGameIds.has(game.id);
              const cover = covers[game.id];
              return (
                <button
                  key={game.id}
                  className="group tile"
                  onClick={() => onOpen(game)}
                  onDoubleClick={() => !running && onLaunch(game)}
                  onContextMenu={(e) => openMenu(e, game)}
                  title={`${game.title} — double-click to play`}
                >
                  <div
                    className="relative aspect-[3/4] w-full"
                    style={cover ? undefined : { background: tileGradient(game.slug) }}
                  >
                    {cover ? (
                      <img
                        src={cover}
                        alt=""
                        className="absolute inset-0 h-full w-full object-cover"
                      />
                    ) : (
                      <span className="absolute inset-0 flex items-center justify-center text-2xl font-bold text-white/30">
                        {initials(game.title)}
                      </span>
                    )}
                    {running && (
                      <span className="absolute inset-x-0 bottom-0 bg-black/70 py-0.5 text-center text-[10px] font-medium text-ok">
                        RUNNING
                      </span>
                    )}
                  </div>
                  <div className="px-2 py-1.5">
                    <p className="truncate text-[12px] font-medium text-t1">{game.title}</p>
                    <p className="truncate text-[11px] text-t3">{playtime(game.play_seconds)}</p>
                  </div>
                </button>
              );
            })}
          </div>
        ) : (
          <table className="w-full">
            <tbody>
              {visible.map((game) => {
                const running = runningGameIds.has(game.id);
                const cover = covers[game.id];
                return (
                  <tr
                    key={game.id}
                    className="row"
                    onClick={() => onOpen(game)}
                    onDoubleClick={() => !running && onLaunch(game)}
                    onContextMenu={(e) => openMenu(e, game)}
                  >
                    <td className="w-10 py-1 pl-3">
                      <div
                        className="h-9 w-7 overflow-hidden rounded-sm"
                        style={cover ? undefined : { background: tileGradient(game.slug) }}
                      >
                        {cover && (
                          <img src={cover} alt="" className="h-full w-full object-cover" />
                        )}
                      </div>
                    </td>
                    <td className="px-3 py-1 text-t1">{game.title}</td>
                    <td className="px-3 py-1 text-[12px] text-t3">
                      {RUNNER_LABELS[game.runner]}
                    </td>
                    <td className="px-3 py-1 text-right text-[12px] text-t3">
                      {running ? <span className="text-ok">running</span> : playtime(game.play_seconds)}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          entries={gameMenu(menu.game)}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
}
