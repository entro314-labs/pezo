import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, playtime } from "../api";
import type { Game } from "../types";
import { RUNNER_LABELS } from "../types";

interface Props {
  games: Game[];
  runningGameIds: Set<number>;
  onLaunch: (game: Game) => void;
  onRemoved: () => void;
  onError: (message: string) => void;
}

/** Deterministic gradient per slug so every tile gets a stable identity color. */
function tileGradient(slug: string): string {
  let hash = 0;
  for (const c of slug) {
    hash = (hash * 31 + c.charCodeAt(0)) | 0;
  }
  const hue = ((hash % 360) + 360) % 360;
  return `linear-gradient(140deg, hsl(${hue} 45% 28%), hsl(${(hue + 40) % 360} 55% 16%))`;
}

function initials(title: string): string {
  return title
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase() ?? "")
    .join("");
}

export function LibraryView({ games, runningGameIds, onLaunch, onRemoved, onError }: Props) {
  if (games.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
        <p className="text-lg text-zinc-400">No games installed yet</p>
        <p className="text-sm text-zinc-600">
          Head to Discoveries, scan your installers folder and install something.
        </p>
      </div>
    );
  }

  return (
    <div className="p-8">
      <h2 className="mb-6 text-xl font-semibold tracking-tight text-white">Library</h2>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-5">
        {games.map((game) => {
          const running = runningGameIds.has(game.id);
          return (
            <div
              key={game.id}
              className="group overflow-hidden rounded-xl border border-ink-700 bg-ink-900 transition-transform hover:-translate-y-0.5"
            >
              <button
                onClick={() => !running && onLaunch(game)}
                className="relative block aspect-[3/4] w-full"
                style={{ background: tileGradient(game.slug) }}
                title={running ? "Running…" : `Play ${game.title}`}
              >
                <span className="absolute inset-0 flex items-center justify-center text-4xl font-bold text-white/25">
                  {initials(game.title)}
                </span>
                <span
                  className={`absolute inset-0 flex items-center justify-center bg-black/50 text-white transition-opacity ${
                    running ? "opacity-100" : "opacity-0 group-hover:opacity-100"
                  }`}
                >
                  {running ? (
                    <span className="text-sm font-medium">Running…</span>
                  ) : (
                    <span className="flex h-12 w-12 items-center justify-center rounded-full bg-accent-600 text-lg">
                      ▶
                    </span>
                  )}
                </span>
              </button>
              <div className="px-3 py-2.5">
                <p className="truncate text-sm font-medium text-zinc-100" title={game.title}>
                  {game.title}
                </p>
                <div className="mt-1 flex items-center justify-between text-xs text-zinc-500">
                  <span>{RUNNER_LABELS[game.runner]}</span>
                  <span>{playtime(game.play_seconds)}</span>
                </div>
                <div className="mt-2 flex gap-2 opacity-0 transition-opacity group-hover:opacity-100">
                  <button
                    onClick={() =>
                      revealItemInDir(game.install_path).catch((e) => onError(String(e)))
                    }
                    className="text-xs text-zinc-500 hover:text-zinc-300"
                  >
                    Files
                  </button>
                  <button
                    onClick={async () => {
                      try {
                        await api.removeGame(game.id);
                        onRemoved();
                      } catch (e) {
                        onError(String(e));
                      }
                    }}
                    className="text-xs text-zinc-500 hover:text-red-400"
                    title="Remove from library (files stay on disk)"
                  >
                    Remove
                  </button>
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
