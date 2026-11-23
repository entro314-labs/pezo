import { useCallback, useEffect, useState } from "react";
import { api, humanSize } from "../api";
import type {
  AppConfig,
  WineBuild,
  WineInstallProgress,
  WineRelease,
  WineSource,
} from "../types";
import { BUILD_SOURCE_LABELS } from "../types";

interface Props {
  config: AppConfig;
  onConfigChanged: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
}

function progressLabel(p: WineInstallProgress): string {
  switch (p.stage) {
    case "downloading":
      return `Downloading ${humanSize(p.done)} of ${humanSize(p.total)}`;
    case "verifying":
      return "Verifying checksum…";
    case "extracting":
      return "Extracting…";
  }
}

/** Installed Wine/Proton builds, the default pick, and downloads of new ones. */
export function WineBuildsSection({ config, onConfigChanged, onError, onNotice }: Props) {
  const [builds, setBuilds] = useState<WineBuild[]>([]);
  const [sources, setSources] = useState<WineSource[]>([]);
  const [sourceId, setSourceId] = useState("");
  const [releases, setReleases] = useState<WineRelease[] | null>(null);
  const [loadingReleases, setLoadingReleases] = useState(false);
  const [installing, setInstalling] = useState<string | null>(null);
  const [progress, setProgress] = useState<WineInstallProgress | null>(null);
  const [confirmingRemove, setConfirmingRemove] = useState<string | null>(null);

  const loadBuilds = useCallback(() => {
    api
      .listWineBuilds()
      .then(setBuilds)
      .catch((e) => onError(String(e)));
  }, [onError]);

  useEffect(() => {
    loadBuilds();
    api
      .wineSources()
      .then((list) => {
        setSources(list);
        setSourceId((current) => current || (list[0]?.id ?? ""));
      })
      .catch((e) => onError(String(e)));
  }, [loadBuilds, onError]);

  useEffect(() => {
    const unlisten = api.onWineInstallProgress((e) => setProgress(e.progress));
    return () => {
      unlisten.then((fn) => fn()).catch(() => undefined);
    };
  }, []);

  const loadReleases = async (id: string) => {
    setLoadingReleases(true);
    setReleases(null);
    try {
      setReleases(await api.listWineReleases(id));
    } catch (e) {
      onError(String(e));
    } finally {
      setLoadingReleases(false);
    }
  };

  const install = async (release: WineRelease) => {
    setInstalling(release.tag);
    setProgress(null);
    try {
      const build = await api.installWineBuild(release.source, release.tag);
      onNotice(`Installed ${build.name}`);
      loadBuilds();
      setReleases((prev) =>
        prev?.map((r) => (r.tag === release.tag ? { ...r, installed: true } : r)) ?? null,
      );
    } catch (e) {
      onError(String(e));
    } finally {
      setInstalling(null);
      setProgress(null);
    }
  };

  const setDefault = async (name: string | null) => {
    try {
      await api.setDefaultWine(name);
      onConfigChanged();
    } catch (e) {
      onError(String(e));
    }
  };

  const remove = async (name: string) => {
    setConfirmingRemove(null);
    try {
      await api.removeWineBuild(name);
      onNotice(`Removed ${name}`);
      loadBuilds();
      onConfigChanged();
    } catch (e) {
      onError(String(e));
    }
  };

  const source = sources.find((s) => s.id === sourceId);
  const fraction =
    progress?.stage === "downloading" && progress.total > 0 ? progress.done / progress.total : null;

  return (
    <section className="card mb-6 p-5">
      <div className="flex items-center justify-between gap-4">
        <h3 className="text-sm font-semibold text-t1">Wine &amp; Proton</h3>
        <button onClick={loadBuilds} className="btn-ghost shrink-0 py-1.5 text-xs">
          Rescan
        </button>
      </div>
      <p className="mt-1 text-xs text-t3">
        Windows games run with the default build unless they pick their own under Launch
        options. Proton runs through umu-run; Wine builds run directly.
      </p>

      <ul className="mt-4 flex flex-col gap-2">
        <li>
          <label className="flex items-start gap-3">
            <input
              type="radio"
              name="default-wine"
              className="mt-1"
              checked={config.default_wine === null}
              onChange={() => setDefault(null)}
            />
            <span className="min-w-0 flex-1">
              <span className="block text-sm text-t1">Automatic (umu)</span>
              <span className="block text-xs text-t3">
                Latest UMU-Proton, downloaded by umu-run on first launch
              </span>
            </span>
          </label>
        </li>
        {builds.map((build) => (
          <li key={build.name} className="flex items-start gap-3">
            <label className="flex min-w-0 flex-1 items-start gap-3">
              <input
                type="radio"
                name="default-wine"
                className="mt-1"
                checked={config.default_wine === build.name}
                onChange={() => setDefault(build.name)}
              />
              <span className="min-w-0 flex-1">
                <span className="flex flex-wrap items-center gap-2 text-sm text-t1">
                  {build.name}
                  <span className="chip bg-accent-soft text-accent">
                    {build.kind === "proton" ? "Proton" : "Wine"}
                  </span>
                  <span className="chip bg-raised text-t2">
                    {BUILD_SOURCE_LABELS[build.source]}
                  </span>
                </span>
                <span className="block truncate text-xs text-t3 select-text" title={build.path}>
                  {build.path}
                </span>
              </span>
            </label>
            {build.removable &&
              (confirmingRemove === build.name ? (
                <div className="flex shrink-0 gap-1">
                  <button onClick={() => remove(build.name)} className="btn-danger h-6 px-2 text-xs">
                    Delete
                  </button>
                  <button
                    onClick={() => setConfirmingRemove(null)}
                    className="btn-ghost h-6 px-2 text-xs"
                  >
                    Cancel
                  </button>
                </div>
              ) : (
                <button
                  onClick={() => setConfirmingRemove(build.name)}
                  className="btn-ghost shrink-0 px-2 py-1 text-xs hover:text-danger"
                >
                  Remove
                </button>
              ))}
          </li>
        ))}
      </ul>

      <div className="mt-5 border-t border-edge pt-4">
        <h4 className="text-xs font-semibold text-t2">Download a build</h4>
        <div className="mt-2 flex items-center gap-2">
          <select
            value={sourceId}
            onChange={(e) => {
              setSourceId(e.target.value);
              setReleases(null);
            }}
            className="input text-t2"
          >
            {sources.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
          <button
            onClick={() => loadReleases(sourceId)}
            disabled={!sourceId || loadingReleases}
            className="btn-secondary shrink-0 py-1.5 text-xs"
          >
            {loadingReleases ? "Loading…" : "Show releases"}
          </button>
        </div>
        {source && <p className="mt-1.5 text-xs text-t3">{source.description}</p>}

        {installing && (
          <div className="mt-3">
            <p className="text-xs text-t2">
              {installing}: {progress ? progressLabel(progress) : "Starting…"}
            </p>
            <div className="mt-1 h-1 overflow-hidden rounded-full bg-edge">
              <div
                className="h-full rounded-full bg-accent transition-all"
                style={{ width: `${Math.round((fraction ?? 1) * 100)}%` }}
              />
            </div>
          </div>
        )}

        {releases && releases.length === 0 && (
          <p className="mt-3 text-xs text-t3">No installable releases found.</p>
        )}
        {releases && releases.length > 0 && (
          <ul className="mt-3 flex max-h-64 flex-col gap-1.5 overflow-y-auto">
            {releases.map((r) => (
              <li key={r.tag} className="flex items-center gap-3">
                <span className="min-w-0 flex-1 truncate text-sm text-t1" title={r.asset}>
                  {r.tag}
                  {r.prerelease && <span className="ml-2 text-xs text-warn">prerelease</span>}
                </span>
                <span className="shrink-0 text-xs text-t3">
                  {humanSize(r.size)}
                  {r.published_at && ` · ${r.published_at.slice(0, 10)}`}
                </span>
                <button
                  onClick={() => install(r)}
                  disabled={r.installed || installing !== null}
                  className="btn-secondary h-6 w-20 shrink-0 px-2.5 text-[12px]"
                >
                  {r.installed ? "Installed" : installing === r.tag ? "Installing…" : "Install"}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}
