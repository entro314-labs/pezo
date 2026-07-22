import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, humanSize, sourceLabel } from "../api";
import type { AppConfig, Candidate, CandidateKind, QueueTask, ScanProgress } from "../types";
import { KIND_LABELS, STAGE_LABELS } from "../types";
import { ContextMenu, type MenuEntry } from "./ContextMenu";
import { IconCopy, IconFolder, IconPlay, IconRefresh, IconSearch } from "./Icons";

interface Props {
  candidates: Candidate[];
  config: AppConfig;
  queue: QueueTask[];
  toolbarSlot: HTMLDivElement | null;
  onChanged: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  onScanDone: (summary: string) => void;
}

const KIND_COLORS: Record<string, string> = {
  "gog-installer": "bg-purple-500/15 text-purple-300",
  "inno-installer": "bg-violet-500/15 text-violet-300",
  "windows-installer": "bg-sky-500/15 text-sky-300",
  "portable-windows": "bg-emerald-500/15 text-emerald-300",
  "dos-game": "bg-amber-500/15 text-amber-300",
  "scummvm-game": "bg-orange-500/15 text-orange-300",
  archive: "bg-raised text-t2",
  unknown: "bg-raised text-t3",
};

const FILTER_KINDS: CandidateKind[] = [
  "gog-installer",
  "inno-installer",
  "windows-installer",
  "portable-windows",
  "dos-game",
  "scummvm-game",
  "archive",
  "unknown",
];

type SortBy = "name" | "size";

interface MenuState {
  x: number;
  y: number;
  candidate: Candidate;
}

export function DiscoveriesView({
  candidates,
  config,
  queue,
  toolbarSlot,
  onChanged,
  onError,
  onNotice,
  onScanDone,
}: Props) {
  const [scanning, setScanning] = useState(false);
  const [scanProgress, setScanProgress] = useState<ScanProgress | null>(null);
  const [query, setQuery] = useState("");
  const [kindFilter, setKindFilter] = useState<CandidateKind | null>(null);
  const [sortBy, setSortBy] = useState<SortBy>("name");
  const [hideInstalled, setHideInstalled] = useState(false);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [pickerFor, setPickerFor] = useState<number | null>(null);
  const [pickerArchives, setPickerArchives] = useState<string[]>([]);
  const [menu, setMenu] = useState<MenuState | null>(null);

  useEffect(() => {
    const unlistenScan = api.onScanProgress(setScanProgress);
    return () => {
      unlistenScan.then((fn) => fn()).catch(() => undefined);
    };
  }, []);

  /** Active queue entry for a candidate, if any. */
  const activeTask = useCallback(
    (candidateId: number): QueueTask | undefined =>
      queue.find(
        (t) =>
          t.candidate_id === candidateId && (t.status === "queued" || t.status === "running"),
      ),
    [queue],
  );

  const duplicateNames = useMemo(() => {
    const counts = new Map<string, number>();
    for (const c of candidates) {
      const key = c.display_name.toLowerCase();
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    return new Set([...counts.entries()].filter(([, n]) => n > 1).map(([k]) => k));
  }, [candidates]);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = candidates.filter((c) => {
      if (kindFilter && c.kind !== kindFilter) return false;
      if (hideInstalled && c.installed_game_id !== null) return false;
      if (q && !c.display_name.toLowerCase().includes(q) && !c.path.toLowerCase().includes(q))
        return false;
      return true;
    });
    return filtered.sort((a, b) =>
      sortBy === "size"
        ? b.size_bytes - a.size_bytes
        : a.display_name.localeCompare(b.display_name, undefined, { sensitivity: "base" }),
    );
  }, [candidates, query, kindFilter, sortBy, hideInstalled]);

  const scan = useCallback(
    async (full: boolean) => {
      setScanning(true);
      setScanProgress(null);
      try {
        const outcome = await api.scan(full);
        const parts = [`Found ${outcome.discovered.length} candidates`];
        if (outcome.pruned > 0) parts.push(`${outcome.pruned} removed`);
        if (outcome.offline.length > 0) {
          parts.push(`${outcome.offline.length} scan folder(s) offline — entries kept`);
        }
        const summary = parts.join(" · ");
        onNotice(summary);
        onScanDone(summary);
        onChanged();
      } catch (e) {
        onError(String(e));
      } finally {
        setScanning(false);
        setScanProgress(null);
      }
    },
    [onChanged, onError, onNotice, onScanDone],
  );

  // F5 anywhere in the app lands here.
  useEffect(() => {
    const onScanEvent = () => void scan(false);
    window.addEventListener("pezo:scan", onScanEvent);
    return () => window.removeEventListener("pezo:scan", onScanEvent);
  }, [scan]);

  const enqueue = (candidate: Candidate, archive?: string) => {
    api.queueInstall(candidate.id, archive).catch((e) => onError(String(e)));
  };

  const openPicker = async (candidate: Candidate) => {
    if (pickerFor === candidate.id) {
      setPickerFor(null);
      return;
    }
    try {
      const archives = await api.listCandidateArchives(candidate.id);
      if (archives.length === 0) {
        onNotice("No extractable archives found in this folder.");
        return;
      }
      setPickerArchives(archives);
      setPickerFor(candidate.id);
    } catch (e) {
      onError(String(e));
    }
  };

  const defaultAction = (c: Candidate) => {
    if (c.installed_game_id !== null || activeTask(c.id)) return;
    if (c.kind === "unknown") {
      void openPicker(c);
    } else {
      enqueue(c);
    }
  };

  const candidateMenu = (c: Candidate): MenuEntry[] => {
    const installed = c.installed_game_id !== null;
    const busy = activeTask(c.id) !== undefined;
    return [
      c.kind === "unknown"
        ? { label: "Pick archive…", disabled: installed || busy, onClick: () => openPicker(c) }
        : {
            label: installed ? "Installed" : busy ? "In queue" : "Install",
            icon: <IconPlay width={14} height={14} />,
            disabled: installed || busy,
            onClick: () => enqueue(c),
          },
      { separator: true },
      {
        label: "Show in file manager",
        icon: <IconFolder width={14} height={14} />,
        onClick: () => revealItemInDir(c.path).catch((e) => onError(String(e))),
      },
      {
        label: "Copy path",
        icon: <IconCopy width={14} height={14} />,
        onClick: () =>
          navigator.clipboard?.writeText(c.path).catch(() => onError("Clipboard unavailable")),
      },
    ];
  };

  const scanLabel = scanning
    ? scanProgress && scanProgress.total > 0
      ? `${scanProgress.done}/${scanProgress.total}`
      : "Scanning…"
    : "Scan";

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
      <button
        onClick={() => scan(true)}
        disabled={scanning}
        title="Re-classify everything, ignoring the incremental cache"
        className="btn-ghost text-[12px]"
      >
        Full rescan
      </button>
      <button
        onClick={() => scan(false)}
        disabled={scanning}
        className="btn-primary"
        title="Scan library and folders (F5)"
      >
        <IconRefresh width={13} height={13} className={scanning ? "animate-spin" : ""} />
        {scanLabel}
      </button>
    </>
  );

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col">
      {toolbarSlot && createPortal(toolbar, toolbarSlot)}

      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-edge px-3">
        <select
          value={sortBy}
          onChange={(e) => setSortBy(e.target.value as SortBy)}
          className="input h-6 text-[12px] text-t2"
        >
          <option value="name">Name</option>
          <option value="size">Size</option>
        </select>
        <select
          value={kindFilter ?? ""}
          onChange={(e) => setKindFilter((e.target.value || null) as CandidateKind | null)}
          className="input h-6 text-[12px] text-t2"
        >
          <option value="">All types</option>
          {FILTER_KINDS.map((kind) => (
            <option key={kind} value={kind}>
              {KIND_LABELS[kind]}
            </option>
          ))}
        </select>
        <label className="flex items-center gap-1.5 text-[12px] text-t2">
          <input
            type="checkbox"
            checked={hideInstalled}
            onChange={(e) => setHideInstalled(e.target.checked)}
          />
          Hide installed
        </label>
        <span className="ml-auto text-[11px] text-t3">
          {visible.length} of {candidates.length}
        </span>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {candidates.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-1.5 text-center">
            <p className="text-sm text-t2">Nothing scanned yet</p>
            <p className="text-[12px] text-t3">
              Drop installers into the library or a scan folder, then Scan.
            </p>
          </div>
        ) : (
          <table className="w-full">
            <thead>
              <tr className="sticky top-0 border-b border-edge bg-surface text-left text-[11px] tracking-wide text-t3 uppercase">
                <th className="px-3 py-1.5 font-medium">Name</th>
                <th className="w-32 px-3 py-1.5 font-medium">Type</th>
                <th className="w-36 px-3 py-1.5 font-medium">Source</th>
                <th className="w-24 px-3 py-1.5 text-right font-medium">Size</th>
                <th className="w-36 px-3 py-1.5 text-right font-medium" />
              </tr>
            </thead>
            <tbody>
              {visible.map((c) => {
                const task = activeTask(c.id);
                const installed = c.installed_game_id !== null;
                const duplicate = duplicateNames.has(c.display_name.toLowerCase());
                const selected = selectedId === c.id;
                return (
                  <Fragment key={c.id}>
                    <tr
                      className={`row ${selected ? "row-selected" : ""}`}
                      onClick={() => setSelectedId(c.id)}
                      onDoubleClick={() => defaultAction(c)}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        setSelectedId(c.id);
                        setMenu({ x: e.clientX, y: e.clientY, candidate: c });
                      }}
                    >
                      <td className="max-w-0 truncate px-3 py-1.5 text-t1" title={c.path}>
                        {c.display_name}
                        {duplicate && <span className="chip ml-2 bg-warn/15 text-warn">dup</span>}
                      </td>
                      <td className="px-3 py-1.5">
                        <span className={`chip ${KIND_COLORS[c.kind] ?? ""}`}>
                          {KIND_LABELS[c.kind]}
                        </span>
                      </td>
                      <td className="truncate px-3 py-1.5 text-[12px] text-t3">
                        {sourceLabel(c.path, config)}
                      </td>
                      <td className="px-3 py-1.5 text-right text-[12px] whitespace-nowrap text-t2">
                        {humanSize(c.size_bytes)}
                      </td>
                      <td className="px-3 py-1.5 text-right whitespace-nowrap">
                        {installed ? (
                          <span className="text-[12px] text-ok">Installed</span>
                        ) : task?.status === "running" ? (
                          <span className="text-[12px] text-accent">
                            {task.progress !== null
                              ? `${Math.round(task.progress * 100)}%`
                              : task.stage
                                ? STAGE_LABELS[task.stage]
                                : "Installing…"}
                          </span>
                        ) : task ? (
                          <span className="text-[12px] text-t3">Queued</span>
                        ) : (
                          <button
                            onClick={(e) => {
                              e.stopPropagation();
                              defaultAction(c);
                            }}
                            className="btn-secondary h-6 px-2.5 text-[12px]"
                          >
                            {c.kind === "unknown" ? "Pick…" : "Install"}
                          </button>
                        )}
                      </td>
                    </tr>
                    {pickerFor === c.id && (
                      <tr className="border-b border-edge bg-raised/60">
                        <td colSpan={5} className="px-3 py-2">
                          <p className="mb-1.5 text-[11px] text-t3">
                            Archives in this folder — pick the one to install:
                          </p>
                          <div className="flex flex-wrap gap-1.5">
                            {pickerArchives.map((archive) => (
                              <button
                                key={archive}
                                onClick={() => {
                                  setPickerFor(null);
                                  enqueue(c, archive);
                                }}
                                className="btn-secondary h-6 px-2.5 text-[12px]"
                              >
                                {archive.split("/").pop()}
                              </button>
                            ))}
                          </div>
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        )}
        {candidates.length > 0 && visible.length === 0 && (
          <p className="p-8 text-center text-[12px] text-t3">
            Nothing matches the current search/filters.
          </p>
        )}
      </div>

      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          entries={candidateMenu(menu.candidate)}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
}
