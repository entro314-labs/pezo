use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use pezo_core::wine::{self, InstallProgress};
use pezo_core::{Config, Db, Game, install, launch, open_library, scan};

#[derive(Parser)]
#[command(
    name = "pezo",
    about = "Offline game library manager: GOG installers, abandonware, DOS classics — installed and launched on Linux",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Set the library root (e.g. /mnt/games) and create its layout
    Init {
        /// Path to the library disk/folder
        path: PathBuf,
    },
    /// Check that external tools (umu, innoextract, dosbox, …) are available
    Doctor,
    /// Scan the installers folder and classify everything found
    Scan {
        /// Directory to scan (defaults to <library>/installers)
        dir: Option<PathBuf>,
        /// Re-classify everything instead of skipping unchanged entries
        #[arg(long)]
        full: bool,
    },
    /// List scanned, installable items
    Candidates,
    /// Install a candidate by id
    Install {
        candidate_id: i64,
        /// Show Windows installers' own UI instead of running them unattended
        #[arg(long)]
        interactive: bool,
    },
    /// List installed games
    Games,
    /// Launch a game by id or slug (blocks until it exits, records playtime)
    Launch {
        /// Game id or slug
        game: String,
    },
    /// Show the current configuration
    Config,
    /// Manage Wine and Proton builds
    Wine {
        #[command(subcommand)]
        command: WineCmd,
    },
}

#[derive(Subcommand)]
enum WineCmd {
    /// List installed Wine/Proton builds
    List,
    /// List downloadable releases of a source (omit it to list the sources)
    Available { source: Option<String> },
    /// Download and install a release (latest stable unless a tag is given)
    Install { source: String, tag: Option<String> },
    /// Delete a build Pezo is allowed to remove
    Remove { name: String },
    /// Set the build games use unless they pick their own (`umu` = umu-run's default)
    Default { name: String },
    /// Pick the build one game runs with (`default` = follow the default)
    Use {
        /// Game id or slug
        game: String,
        name: String,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pezo_core=info,pezo_cli=info".into()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Cmd::Init { path } => cmd_init(path),
        Cmd::Doctor => cmd_doctor(),
        Cmd::Scan { dir, full } => cmd_scan(dir, full),
        Cmd::Candidates => cmd_candidates(),
        Cmd::Install {
            candidate_id,
            interactive,
        } => cmd_install(candidate_id, interactive),
        Cmd::Games => cmd_games(),
        Cmd::Launch { game } => cmd_launch(&game),
        Cmd::Config => cmd_config(),
        Cmd::Wine { command } => cmd_wine(command),
    }
}

fn cmd_init(path: PathBuf) -> anyhow::Result<()> {
    let root = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .context("cannot resolve current directory")?
            .join(path)
    };
    let mut config = Config::load()?;
    config.library_root = Some(root.clone());
    config.library()?.ensure_layout()?;
    config.save()?;
    println!("library initialized at {}", root.display());
    println!(
        "  drop installers into {}/installers, then run `pezo scan`",
        root.display()
    );
    Ok(())
}

fn cmd_doctor() -> anyhow::Result<()> {
    let mut missing_required = false;
    for tool in pezo_core::deps::doctor() {
        let mark = if tool.found {
            "ok "
        } else if tool.required {
            "MISS"
        } else {
            "opt "
        };
        let location = tool
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| tool.hint.clone());
        println!("[{mark}] {:<20} {} — {}", tool.name, tool.purpose, location);
        if tool.required && !tool.found {
            missing_required = true;
        }
    }
    if missing_required {
        bail!("required tools are missing — install them and re-run `pezo doctor`");
    }
    Ok(())
}

fn cmd_scan(dir: Option<PathBuf>, full: bool) -> anyhow::Result<()> {
    let config = Config::load()?;
    let (paths, db) = open_library(&config)?;
    let opts = scan::ScanOptions {
        full,
        progress: None,
    };
    let (target, outcome) = match dir {
        Some(dir) => {
            let outcome = scan::scan_dir(&db, &dir, &opts)?;
            (dir, outcome)
        }
        None => {
            let outcome = scan::scan_library(&db, &paths, &config.scan_dirs, &opts)?;
            (paths.root.clone(), outcome)
        }
    };
    println!(
        "scanned {}: {} candidates, {} ignored, {} pruned",
        target.display(),
        outcome.discovered.len(),
        outcome.ignored,
        outcome.pruned
    );
    for dir in &outcome.offline {
        println!("  offline: {} (skipped, entries kept)", dir.display());
    }
    for c in &outcome.discovered {
        println!(
            "  #{:<4} {:<18} {:>9}  {}",
            c.id,
            c.kind.as_str(),
            human_size(c.size_bytes),
            c.display_name
        );
    }
    Ok(())
}

fn cmd_candidates() -> anyhow::Result<()> {
    let config = Config::load()?;
    let (_, db) = open_library(&config)?;
    let candidates = db.list_candidates()?;
    if candidates.is_empty() {
        println!("no candidates — run `pezo scan` first");
        return Ok(());
    }
    for c in candidates {
        let installed = match c.installed_game_id {
            Some(id) => format!("installed (game #{id})"),
            None => "not installed".to_string(),
        };
        println!(
            "#{:<4} {:<18} {:>9}  {:<40} {}",
            c.id,
            c.kind.as_str(),
            human_size(c.size_bytes),
            c.display_name,
            installed
        );
    }
    Ok(())
}

fn cmd_install(candidate_id: i64, interactive: bool) -> anyhow::Result<()> {
    let config = Config::load()?;
    let (paths, db) = open_library(&config)?;
    let opts = install::InstallOptions {
        adopt_portables: config.adopt_portables,
        progress: Some(&|stage| println!("… {}", stage.as_str())),
        default_wine: config.default_wine.as_deref(),
        interactive: interactive || config.interactive_installers,
    };
    let game = install::install_candidate(&db, &paths, candidate_id, &opts)?;
    let exe = std::env::current_exe().context("cannot locate the pezo binary")?;
    let exec = format!("\"{}\" launch {}", exe.display(), game.slug);
    if let Some(entry) = install::finish_install(&paths, &game, &exec) {
        println!("desktop launcher: {}", entry.display());
    }
    println!(
        "installed `{}` (#{}) — runner: {}, launch: {}",
        game.title,
        game.id,
        game.runner.as_str(),
        game.launch_path.display()
    );
    println!("run it with: pezo launch {}", game.slug);
    Ok(())
}

fn cmd_games() -> anyhow::Result<()> {
    let config = Config::load()?;
    let (_, db) = open_library(&config)?;
    let games = db.list_games()?;
    if games.is_empty() {
        println!("no installed games — `pezo scan` then `pezo install <id>`");
        return Ok(());
    }
    for g in games {
        let played = if g.play_seconds > 0 {
            format!(
                "{}h{:02}m",
                g.play_seconds / 3600,
                (g.play_seconds % 3600) / 60
            )
        } else {
            "never played".to_string()
        };
        println!(
            "#{:<4} {:<10} {:<40} {}  [{}]",
            g.id,
            g.runner.as_str(),
            g.title,
            played,
            g.slug
        );
    }
    Ok(())
}

fn find_game(db: &Db, game_ref: &str) -> anyhow::Result<Game> {
    Ok(match game_ref.parse::<i64>() {
        Ok(id) => db.get_game(id)?,
        Err(_) => db
            .find_game_by_slug(game_ref)?
            .with_context(|| format!("no game with slug `{game_ref}`"))?,
    })
}

fn cmd_launch(game_ref: &str) -> anyhow::Result<()> {
    let config = Config::load()?;
    let (_, db) = open_library(&config)?;
    let game = find_game(&db, game_ref)?;
    let outcome = launch::launch_game(&db, game.id, config.default_wine.as_deref())?;
    println!(
        "`{}` exited (code {:?}) after {}m{:02}s",
        game.title,
        outcome.exit_code,
        outcome.seconds / 60,
        outcome.seconds % 60
    );
    Ok(())
}

fn cmd_config() -> anyhow::Result<()> {
    let config = Config::load()?;
    println!("config file: {}", Config::path()?.display());
    println!("{}", serde_json::to_string_pretty(&config)?);
    if let Some(root) = &config.library_root
        && let Some(fs) = pezo_core::config::fs_info(root)
    {
        print!("library filesystem: {}", fs.fstype);
        if fs.problematic {
            print!(" — WARNING: Proton/Wine prefixes misbehave on this filesystem");
        }
        println!();
    }
    Ok(())
}

fn cmd_wine(command: WineCmd) -> anyhow::Result<()> {
    let mut config = Config::load()?;
    match command {
        WineCmd::List => {
            let default = config.default_wine.as_deref();
            let mark = if default.is_none() { "*" } else { " " };
            println!("{mark} umu (latest UMU-Proton, downloaded by umu-run)");
            for b in wine::detect()? {
                let mark = if default == Some(b.name.as_str()) {
                    "*"
                } else {
                    " "
                };
                println!(
                    "{mark} {:<36} {:<7} {}",
                    b.name,
                    b.kind.as_str(),
                    b.path.display()
                );
            }
        }
        WineCmd::Available { source: None } => {
            for s in wine::SOURCES {
                println!("{:<18} {}", s.id, s.description);
            }
        }
        WineCmd::Available {
            source: Some(source),
        } => {
            for r in wine::list_releases(&source)? {
                println!(
                    "{:<36} {:>9}  {}{}",
                    r.tag,
                    human_size(r.size),
                    r.published_at
                        .as_deref()
                        .unwrap_or("")
                        .get(..10)
                        .unwrap_or(""),
                    match (r.installed, r.prerelease) {
                        (true, _) => "  installed",
                        (false, true) => "  prerelease",
                        _ => "",
                    }
                );
            }
        }
        WineCmd::Install { source, tag } => {
            let report = |p: InstallProgress| match p {
                InstallProgress::Downloading { done, total } => {
                    eprint!(
                        "\r… downloading {} / {}   ",
                        human_size(done),
                        human_size(total)
                    );
                }
                InstallProgress::Verifying => eprintln!("\n… verifying checksum"),
                InstallProgress::Extracting => eprintln!("… extracting"),
            };
            let build = wine::install_release(&source, tag.as_deref(), &report)?;
            println!("installed {} at {}", build.name, build.path.display());
        }
        WineCmd::Remove { name } => {
            wine::remove(&mut config, &name)?;
            println!("removed {name}");
        }
        WineCmd::Default { name } => {
            let name = (name != "umu").then_some(name);
            wine::set_default(&mut config, name.as_deref())?;
            println!("default: {}", name.as_deref().unwrap_or("umu"));
        }
        WineCmd::Use { game, name } => {
            let (_, db) = open_library(&config)?;
            let game = find_game(&db, &game)?;
            let name = (name != "default").then_some(name);
            if let Some(name) = &name {
                wine::find(name)?;
            }
            db.update_launch_options(
                game.id,
                &game.launch_args,
                &game.env_vars,
                &game.wrappers,
                name.as_deref(),
            )?;
            println!(
                "`{}` now runs with {}",
                game.title,
                name.as_deref().unwrap_or("the default build")
            );
        }
    }
    Ok(())
}

fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
