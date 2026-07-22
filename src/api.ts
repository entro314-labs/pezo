import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";
import { mockEnabled, mockInvoke, mockListen } from "./mock";
import type {
  AppConfig,
  Candidate,
  DesktopLayout,
  FsInfo,
  Game,
  GameExited,
  QueueTask,
  ScanOutcome,
  ScanProgress,
  ToolStatus,
} from "./types";

const invoke: typeof tauriInvoke = mockEnabled ? mockInvoke : tauriInvoke;
const listen: typeof tauriListen = mockEnabled
  ? ((async (event: string, handler: (e: { payload: unknown }) => void) =>
      mockListen(event, handler)) as unknown as typeof tauriListen)
  : tauriListen;

export const api = {
  getConfig: () => invoke<AppConfig>("get_config"),
  setLibraryRoot: (path: string) => invoke<AppConfig>("set_library_root", { path }),
  addScanDir: (path: string) => invoke<AppConfig>("add_scan_dir", { path }),
  removeScanDir: (path: string) => invoke<AppConfig>("remove_scan_dir", { path }),
  setAdoptPortables: (enabled: boolean) =>
    invoke<AppConfig>("set_adopt_portables", { enabled }),
  doctor: () => invoke<ToolStatus[]>("doctor"),
  desktopLayout: () => invoke<DesktopLayout>("desktop_layout"),
  installableTools: () => invoke<string[]>("installable_tools"),
  installTool: (tool: string) => invoke<void>("install_tool", { tool }),
  libraryFsInfo: () => invoke<FsInfo | null>("library_fs_info"),
  scan: (full = false) => invoke<ScanOutcome>("scan_installers", { full }),
  listCandidates: () => invoke<Candidate[]>("list_candidates"),
  listCandidateArchives: (candidateId: number) =>
    invoke<string[]>("list_candidate_archives", { candidateId }),
  listGames: () => invoke<Game[]>("list_games"),
  queueInstall: (candidateId: number, archive?: string) =>
    invoke<number>("queue_install", { candidateId, archive: archive ?? null }),
  queueState: () => invoke<QueueTask[]>("queue_state"),
  queueCancel: (taskId: number) => invoke<boolean>("queue_cancel", { taskId }),
  queueClearFinished: () => invoke<void>("queue_clear_finished"),
  getCover: (gameId: number) => invoke<string | null>("get_cover", { gameId }),
  fetchMissingCovers: () => invoke<number>("fetch_missing_covers"),
  updateLaunchOptions: (
    gameId: number,
    launchArgs: string,
    envVars: string,
    wrappers: string,
  ) => invoke<Game>("update_launch_options", { gameId, launchArgs, envVars, wrappers }),
  runWinetricks: (gameId: number) => invoke<void>("run_winetricks", { gameId }),
  launch: (gameId: number) => invoke<void>("launch_game", { gameId }),
  removeGame: (gameId: number, deleteFiles: boolean) =>
    invoke<void>("remove_game", { gameId, deleteFiles }),
  onGameExited: (handler: (e: GameExited) => void): Promise<UnlistenFn> =>
    listen<GameExited>("game-exited", (event) => handler(event.payload)),
  onScanProgress: (handler: (e: ScanProgress) => void): Promise<UnlistenFn> =>
    listen<ScanProgress>("scan-progress", (event) => handler(event.payload)),
  onQueueChanged: (handler: (tasks: QueueTask[]) => void): Promise<UnlistenFn> =>
    listen<QueueTask[]>("queue-changed", (event) => handler(event.payload)),
};

export function humanSize(bytes: number): string {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${bytes} B` : `${value.toFixed(1)} ${units[unit]}`;
}

export function playtime(seconds: number): string {
  if (seconds <= 0) return "Never played";
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h === 0 && m === 0) return "Under a minute";
  if (h === 0) return `${m}m played`;
  return `${h}h ${m.toString().padStart(2, "0")}m played`;
}

/** Which configured source (library or scan dir) a candidate path lives under. */
export function sourceLabel(path: string, config: AppConfig): string {
  const roots = [
    ...(config.library_root ? [config.library_root] : []),
    ...config.scan_dirs,
  ];
  let best: string | null = null;
  for (const root of roots) {
    if (path.startsWith(root.endsWith("/") ? root : root + "/") || path === root) {
      if (best === null || root.length > best.length) best = root;
    }
  }
  if (best === null) return "";
  const name = best.split("/").filter(Boolean).pop() ?? best;
  return best === config.library_root ? "library" : name;
}
