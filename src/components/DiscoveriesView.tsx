import { useState } from "react";
import { api, humanSize } from "../api";
import type { Candidate, Game } from "../types";
import { KIND_LABELS } from "../types";

interface Props {
  candidates: Candidate[];
  onChanged: () => void;
  onError: (message: string) => void;
  onInstalled: (game: Game) => void;
}

const KIND_COLORS: Record<string, string> = {
  "gog-installer": "bg-purple-500/15 text-purple-300",
  "inno-installer": "bg-violet-500/15 text-violet-300",
  "windows-installer": "bg-sky-500/15 text-sky-300",
  "portable-windows": "bg-emerald-500/15 text-emerald-300",
  "dos-game": "bg-amber-500/15 text-amber-300",
  "scummvm-game": "bg-orange-500/15 text-orange-300",
  archive: "bg-zinc-500/15 text-zinc-300",
  unknown: "bg-zinc-600/15 text-zinc-500",
};

export function DiscoveriesView({ candidates, onChanged, onError, onInstalled }: Props) {
  const [scanning, setScanning] = useState(false);
  const [installingIds, setInstallingIds] = useState<Set<number>>(new Set());

  const scan = async () => {
    setScanning(true);
    try {
      await api.scan();
      onChanged();
    } catch (e) {
      onError(String(e));
    } finally {
      setScanning(false);
    }
  };

  const install = async (candidate: Candidate) => {
    setInstallingIds((prev) => new Set(prev).add(candidate.id));
    try {
      const game = await api.install(candidate.id);
      onInstalled(game);
      onChanged();
    } catch (e) {
      onError(String(e));
    } finally {
      setInstallingIds((prev) => {
        const next = new Set(prev);
        next.delete(candidate.id);
        return next;
      });
    }
  };

  return (
    <div className="p-8">
      <div className="mb-6 flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold tracking-tight text-white">Discoveries</h2>
          <p className="mt-1 text-sm text-zinc-500">
            Everything found in your <code>installers/</code> folder, classified.
          </p>
        </div>
        <button
          onClick={scan}
          disabled={scanning}
          className="rounded-lg bg-accent-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-accent-500 disabled:opacity-50"
        >
          {scanning ? "Scanning…" : "Scan now"}
        </button>
      </div>

      {candidates.length === 0 ? (
        <div className="rounded-xl border border-dashed border-ink-700 p-10 text-center text-sm text-zinc-500">
          Nothing scanned yet. Drop GOG installers, game folders or archives into your
          library's <code>installers/</code> folder and hit Scan.
        </div>
      ) : (
        <div className="overflow-hidden rounded-xl border border-ink-700">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-ink-700 bg-ink-900 text-left text-xs uppercase tracking-wide text-zinc-500">
                <th className="px-4 py-3 font-medium">Name</th>
                <th className="px-4 py-3 font-medium">Type</th>
                <th className="px-4 py-3 text-right font-medium">Size</th>
                <th className="px-4 py-3 text-right font-medium">Action</th>
              </tr>
            </thead>
            <tbody>
              {candidates.map((c) => {
                const installing = installingIds.has(c.id);
                const installed = c.installed_game_id !== null;
                const installable = !installed && c.kind !== "unknown";
                return (
                  <tr key={c.id} className="border-b border-ink-800 last:border-0 hover:bg-ink-900/60">
                    <td className="max-w-md truncate px-4 py-3 text-zinc-200" title={c.path}>
                      {c.display_name}
                    </td>
                    <td className="px-4 py-3">
                      <span
                        className={`rounded-full px-2.5 py-1 text-xs font-medium ${KIND_COLORS[c.kind] ?? ""}`}
                      >
                        {KIND_LABELS[c.kind]}
                      </span>
                    </td>
                    <td className="px-4 py-3 text-right text-zinc-500">
                      {humanSize(c.size_bytes)}
                    </td>
                    <td className="px-4 py-3 text-right">
                      {installed ? (
                        <span className="text-xs text-emerald-400">Installed</span>
                      ) : installable ? (
                        <button
                          onClick={() => install(c)}
                          disabled={installing}
                          className="rounded-md bg-ink-700 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-accent-600 disabled:opacity-50"
                        >
                          {installing ? "Installing…" : "Install"}
                        </button>
                      ) : (
                        <span className="text-xs text-zinc-600">No strategy</span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
