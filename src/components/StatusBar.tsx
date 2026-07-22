import type { FsInfo, QueueTask, ScanProgress } from "../types";
import { STAGE_LABELS } from "../types";
import { IconWarning } from "./Icons";

interface Props {
  gameCount: number;
  discoveryCount: number;
  runningCount: number;
  queue: QueueTask[];
  queueOpen: boolean;
  onToggleQueue: () => void;
  scanProgress: ScanProgress | null;
  fsInfo: FsInfo | null;
  libraryRoot: string | null;
}

export function StatusBar({
  gameCount,
  discoveryCount,
  runningCount,
  queue,
  queueOpen,
  onToggleQueue,
  scanProgress,
  fsInfo,
  libraryRoot,
}: Props) {
  const active = queue.find((t) => t.status === "running");
  const queuedCount = queue.filter((t) => t.status === "queued").length;
  const failedCount = queue.filter((t) => t.status === "failed").length;

  const queueLabel = active
    ? `${active.title}: ${
        active.progress !== null
          ? `${Math.round(active.progress * 100)}%`
          : active.stage
            ? STAGE_LABELS[active.stage]
            : "working…"
      }${queuedCount > 0 ? ` · ${queuedCount} queued` : ""}`
    : queuedCount > 0
      ? `${queuedCount} queued`
      : failedCount > 0
        ? `${failedCount} failed`
        : null;

  const scanning =
    scanProgress !== null
      ? scanProgress.total > 0
        ? `Scanning ${scanProgress.done}/${scanProgress.total}`
        : "Scanning…"
      : null;

  return (
    <footer className="statusbar">
      <span>
        {gameCount} games · {discoveryCount} discoveries
        {runningCount > 0 && <span className="text-ok"> · {runningCount} running</span>}
      </span>
      {libraryRoot && (
        <span className="flex min-w-0 items-center gap-1.5 truncate">
          {fsInfo?.problematic && (
            <IconWarning width={12} height={12} className="shrink-0 text-warn" />
          )}
          <span className="truncate">
            {libraryRoot}
            {fsInfo ? ` (${fsInfo.fstype})` : ""}
          </span>
        </span>
      )}
      <span className="ml-auto flex items-center gap-3">
        {scanning && <span>{scanning}</span>}
        <button
          onClick={onToggleQueue}
          className={`flex h-full items-center gap-2 rounded px-1.5 transition-colors hover:bg-raised hover:text-t1 ${
            queueOpen ? "text-t1" : ""
          } ${failedCount > 0 && !active ? "text-danger" : ""}`}
          title="Install queue"
        >
          {active && active.progress !== null && (
            <span className="h-1 w-20 overflow-hidden rounded-full bg-edge">
              <span
                className="block h-full rounded-full bg-accent transition-all"
                style={{ width: `${Math.round(active.progress * 100)}%` }}
              />
            </span>
          )}
          {queueLabel ?? "Queue"}
        </button>
      </span>
    </footer>
  );
}
