import { useEffect, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, playtime } from "../api";
import type { Game, WineBuild } from "../types";
import { RUNNER_LABELS } from "../types";

interface Props {
  game: Game;
  cover: string | null;
  running: boolean;
  /** Configured default Wine/Proton build; null → umu's own default. */
  defaultWine: string | null;
  onLaunch: (game: Game) => void;
  onBack: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
}

function tileGradient(slug: string): string {
  let hash = 0;
  for (const c of slug) {
    hash = (hash * 31 + c.charCodeAt(0)) | 0;
  }
  const hue = ((hash % 360) + 360) % 360;
  return `linear-gradient(140deg, hsl(${hue} 45% 38%), hsl(${(hue + 40) % 360} 55% 22%))`;
}

function lastPlayed(game: Game): string {
  if (!game.last_played_at) return "Never played";
  const date = new Date(game.last_played_at + "Z");
  return `Last played ${date.toLocaleDateString()}`;
}

export function GameDetail({
  game,
  cover,
  running,
  defaultWine,
  onLaunch,
  onBack,
  onChanged,
  onError,
  onNotice,
}: Props) {
  const [confirmingRemove, setConfirmingRemove] = useState(false);

  const remove = async (deleteFiles: boolean) => {
    setConfirmingRemove(false);
    try {
      await api.removeGame(game.id, deleteFiles);
      onNotice(
        deleteFiles
          ? `Removed ${game.title} and deleted its installed files`
          : `Removed ${game.title} from the library`,
      );
      onBack();
      onChanged();
    } catch (e) {
      onError(String(e));
    }
  };

  return (
    <div className="relative min-h-0 flex-1 overflow-y-auto">
      {/* Hero backdrop: the game's own art, blurred into atmosphere. */}
      <div className="absolute inset-x-0 top-0 h-72 overflow-hidden" aria-hidden>
        {cover ? (
          <img
            src={cover}
            alt=""
            className="h-full w-full scale-110 object-cover opacity-25 blur-2xl"
          />
        ) : (
          <div
            className="h-full w-full opacity-30"
            style={{ background: tileGradient(game.slug) }}
          />
        )}
        <div
          className="absolute inset-0"
          style={{
            background:
              "linear-gradient(to bottom, rgba(14,12,10,0.2), rgba(14,12,10,0.85) 70%, var(--bg))",
          }}
        />
      </div>

      <div className="relative mx-auto max-w-3xl p-6">
        <button onClick={onBack} className="btn-ghost -ml-2 mb-4">
          ← Library
        </button>

        <div className="flex gap-6">
          <div
            className="h-60 w-44 shrink-0 overflow-hidden rounded-lg border border-edge"
            style={{
              boxShadow: "var(--shadow)",
              ...(cover ? {} : { background: tileGradient(game.slug) }),
            }}
          >
            {cover && <img src={cover} alt="" className="h-full w-full object-cover" />}
          </div>

          <div className="min-w-0 flex-1">
            <h2 className="text-3xl font-bold tracking-tight text-t1">{game.title}</h2>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <span className="chip bg-accent-soft text-accent">{RUNNER_LABELS[game.runner]}</span>
            {game.store && <span className="chip bg-raised text-t2">{game.store.toUpperCase()}</span>}
            {game.store_id && (
              <span className="chip bg-raised text-t2" title="Store id used for umu protonfixes">
                #{game.store_id}
              </span>
            )}
          </div>
          <p className="mt-3 text-sm text-t2">
            {playtime(game.play_seconds)} · {lastPlayed(game)}
          </p>

          <div className="mt-5 flex items-center gap-2.5">
            <button
              onClick={() => !running && onLaunch(game)}
              disabled={running}
              className="btn-primary h-8 px-6 text-sm"
            >
              {running ? "Running…" : "▶ Play"}
            </button>
            <button
              onClick={() => revealItemInDir(game.install_path).catch((e) => onError(String(e)))}
              className="btn-secondary"
              title={game.install_path}
            >
              Files
            </button>
            {game.prefix_path && (
              <button
                onClick={() =>
                  revealItemInDir(game.prefix_path!).catch((e) => onError(String(e)))
                }
                className="btn-secondary"
                title={game.prefix_path}
              >
                Prefix
              </button>
            )}
            {game.runner === "proton" && (
              <button
                onClick={() =>
                  api
                    .runWinetricks(game.id)
                    .then(() => onNotice("Winetricks opening…"))
                    .catch((e) => onError(String(e)))
                }
                className="btn-secondary"
                title="Install runtimes (DXVK, fonts, VC++) into this game's prefix"
              >
                Winetricks
              </button>
            )}
          </div>
        </div>
      </div>

      <LaunchOptions
        game={game}
        defaultWine={defaultWine}
        onChanged={onChanged}
        onError={onError}
        onNotice={onNotice}
      />

      <section className="card mt-4 p-4">
        <h3 className="text-sm font-semibold text-t1">Locations</h3>
        <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5 text-sm">
          <dt className="text-t3">Installed at</dt>
          <dd className="truncate text-t2 select-text" title={game.install_path}>
            {game.install_path}
          </dd>
          <dt className="text-t3">Launches</dt>
          <dd className="truncate text-t2 select-text" title={game.launch_path}>
            {game.launch_path}
          </dd>
          {game.source_path && (
            <>
              <dt className="text-t3">Installed from</dt>
              <dd className="truncate text-t2 select-text" title={game.source_path}>
                {game.source_path}
              </dd>
            </>
          )}
        </dl>
      </section>

      <section className="card mt-4 border-danger/25 p-4">
        <h3 className="text-sm font-semibold text-t1">Remove</h3>
        {confirmingRemove ? (
          <div className="mt-3 flex flex-wrap items-center gap-3">
            <button onClick={() => remove(false)} className="btn-secondary">
              Remove entry only
            </button>
            <button
              onClick={() => remove(true)}
              className="btn-danger"
              title="Deletes the installed copy and wine prefix. Originals outside the library are kept."
            >
              Remove and delete files
            </button>
            <button onClick={() => setConfirmingRemove(false)} className="btn-ghost">
              Cancel
            </button>
          </div>
        ) : (
          <div className="mt-3 flex items-center justify-between gap-4">
            <p className="text-xs text-t3">
              Removing the entry keeps every file. Deleting files clears the installed copy and
              prefix — never the original installer.
            </p>
            <button onClick={() => setConfirmingRemove(true)} className="btn-danger shrink-0">
              Remove…
            </button>
          </div>
        )}
      </section>
      </div>
    </div>
  );
}

function LaunchOptions({
  game,
  defaultWine,
  onChanged,
  onError,
  onNotice,
}: {
  game: Game;
  defaultWine: string | null;
  onChanged: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
}) {
  const wrapperSet = new Set(
    game.wrappers
      .split(",")
      .map((w) => w.trim())
      .filter(Boolean),
  );
  const [args, setArgs] = useState(game.launch_args);
  const [env, setEnv] = useState(game.env_vars);
  const [gamemode, setGamemode] = useState(wrapperSet.has("gamemode"));
  const [mangohud, setMangohud] = useState(wrapperSet.has("mangohud"));
  const [wineBuild, setWineBuild] = useState(game.wine_build);
  const [builds, setBuilds] = useState<WineBuild[]>([]);
  const [saving, setSaving] = useState(false);
  const isWindows = game.runner === "proton";

  useEffect(() => {
    if (!isWindows) return;
    api
      .listWineBuilds()
      .then(setBuilds)
      .catch((e) => onError(String(e)));
  }, [isWindows, onError]);

  const dirty =
    args !== game.launch_args ||
    env !== game.env_vars ||
    gamemode !== wrapperSet.has("gamemode") ||
    mangohud !== wrapperSet.has("mangohud") ||
    wineBuild !== game.wine_build;

  const save = async () => {
    setSaving(true);
    try {
      const wrappers = [gamemode && "gamemode", mangohud && "mangohud"]
        .filter(Boolean)
        .join(",");
      await api.updateLaunchOptions(game.id, args, env, wrappers, wineBuild);
      onNotice("Launch options saved");
      onChanged();
    } catch (e) {
      onError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="card mt-6 p-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-t1">Launch options</h3>
        {dirty && (
          <button onClick={save} disabled={saving} className="btn-primary px-4 py-1.5 text-xs">
            {saving ? "Saving…" : "Save"}
          </button>
        )}
      </div>

      {isWindows && (
        <label className="mt-4 block text-xs font-medium text-t2">
          Wine / Proton build
          <select
            value={wineBuild ?? ""}
            onChange={(e) => setWineBuild(e.target.value || null)}
            className="input mt-1.5 w-full"
          >
            <option value="">Default ({defaultWine ?? "automatic — umu picks UMU-Proton"})</option>
            {builds.map((b) => (
              <option key={b.name} value={b.name}>
                {b.name} · {b.kind === "proton" ? "Proton" : "Wine"}
              </option>
            ))}
            {wineBuild && !builds.some((b) => b.name === wineBuild) && (
              <option value={wineBuild}>{wineBuild} · not installed on this machine</option>
            )}
          </select>
        </label>
      )}

      <div className="mt-4 grid gap-4 sm:grid-cols-2">
        <label className="block text-xs font-medium text-t2">
          Extra arguments
          <input
            value={args}
            onChange={(e) => setArgs(e.target.value)}
            placeholder="-windowed -skipintro"
            className="input mt-1.5 w-full"
          />
        </label>
        <div className="flex flex-col justify-end gap-2 pb-1">
          <label className="flex items-center gap-2 text-sm text-t1">
            <input
              type="checkbox"
              checked={gamemode}
              onChange={(e) => setGamemode(e.target.checked)}
            />
            GameMode
            <span className="text-xs text-t3">performance governor</span>
          </label>
          <label className="flex items-center gap-2 text-sm text-t1">
            <input
              type="checkbox"
              checked={mangohud}
              onChange={(e) => setMangohud(e.target.checked)}
            />
            MangoHud
            <span className="text-xs text-t3">FPS overlay</span>
          </label>
        </div>
      </div>

      <label className="mt-4 block text-xs font-medium text-t2">
        Environment (one KEY=VALUE per line)
        <textarea
          value={env}
          onChange={(e) => setEnv(e.target.value)}
          rows={3}
          placeholder={"PROTON_USE_WINED3D=1\nDXVK_HUD=fps"}
          className="input mt-1.5 w-full font-mono text-xs"
        />
      </label>
    </section>
  );
}
