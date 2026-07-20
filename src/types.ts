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
}

export interface ScanOutcome {
  discovered: Candidate[];
  ignored: number;
  pruned: number;
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

export const RUNNER_LABELS: Record<Runner, string> = {
  proton: "Proton",
  dosbox: "DOSBox",
  scummvm: "ScummVM",
};
