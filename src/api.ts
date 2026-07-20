import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppConfig,
  Candidate,
  Game,
  GameExited,
  ScanOutcome,
  ToolStatus,
} from "./types";

export const api = {
  getConfig: () => invoke<AppConfig>("get_config"),
  setLibraryRoot: (path: string) => invoke<AppConfig>("set_library_root", { path }),
  addScanDir: (path: string) => invoke<AppConfig>("add_scan_dir", { path }),
  removeScanDir: (path: string) => invoke<AppConfig>("remove_scan_dir", { path }),
  doctor: () => invoke<ToolStatus[]>("doctor"),
  scan: () => invoke<ScanOutcome>("scan_installers"),
  listCandidates: () => invoke<Candidate[]>("list_candidates"),
  listGames: () => invoke<Game[]>("list_games"),
  install: (candidateId: number) => invoke<Game>("install_candidate", { candidateId }),
  launch: (gameId: number) => invoke<void>("launch_game", { gameId }),
  removeGame: (gameId: number) => invoke<void>("remove_game", { gameId }),
  onGameExited: (handler: (e: GameExited) => void): Promise<UnlistenFn> =>
    listen<GameExited>("game-exited", (event) => handler(event.payload)),
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
