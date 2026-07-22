// Mirrors the serde shapes in pezo-core (kebab-case enums, camelCase-free structs).

export type CandidateKind =
  | "gog-installer"
  | "inno-installer"
  | "windows-installer"
  | "portable-windows"
  | "dos-game"
  | "scummvm-game"
  | "archive"
  | "unknown";

export type Runner = "proton" | "dosbox" | "scummvm";

export interface AppConfig {
  library_root: string | null;
  scan_dirs: string[];
  adopt_portables: boolean;
}

export interface FsInfo {
  fstype: string;
  problematic: boolean;
}

/** Window-button layout mirrored from GNOME settings. */
export interface DesktopLayout {
  left: string[];
  right: string[];
}

export interface Candidate {
  id: number;
  path: string;
  kind: CandidateKind;
  display_name: string;
  size_bytes: number;
  discovered_at: string;
  installed_game_id: number | null;
}

export interface Game {
  id: number;
  slug: string;
  title: string;
  runner: Runner;
  install_path: string;
  launch_path: string;
  prefix_path: string | null;
  source_path: string | null;
  added_at: string;
  last_played_at: string | null;
  play_seconds: number;
  store_id: string | null;
  store: string | null;
  launch_args: string;
  env_vars: string;
  wrappers: string;
}

export interface ScanOutcome {
  discovered: Candidate[];
  ignored: number;
  pruned: number;
  /** Scan folders skipped because they're currently missing (unplugged disk). */
  offline: string[];
}

export interface ScanProgress {
  done: number;
  total: number;
}

export type InstallStage = "extracting" | "copying" | "running-installer" | "registering";

/** One entry in the sequential install queue. */
export interface QueueTask {
  id: number;
  candidate_id: number;
  title: string;
  archive: string | null;
  status: "queued" | "running" | "done" | "failed" | "cancelled";
  stage: InstallStage | null;
  /** 0..1 while bytes are landing in the library and the total is known. */
  progress: number | null;
  error: string | null;
  game_id: number | null;
}

export interface ToolStatus {
  name: string;
  found: boolean;
  path: string | null;
  purpose: string;
  hint: string;
  required: boolean;
}

export interface GameExited {
  outcome: { game_id: number; exit_code: number | null; seconds: number } | null;
  error: string | null;
  game_id: number;
}

export const KIND_LABELS: Record<CandidateKind, string> = {
  "gog-installer": "GOG installer",
  "inno-installer": "Inno installer",
  "windows-installer": "Windows installer",
  "portable-windows": "Portable",
  "dos-game": "DOS",
  "scummvm-game": "ScummVM",
  archive: "Archive",
  unknown: "Unknown",
};

export const STAGE_LABELS: Record<InstallStage, string> = {
  extracting: "Extracting…",
  copying: "Copying…",
  "running-installer": "Installer running…",
  registering: "Registering…",
};

export const RUNNER_LABELS: Record<Runner, string> = {
  proton: "Proton",
  dosbox: "DOSBox",
  scummvm: "ScummVM",
};
