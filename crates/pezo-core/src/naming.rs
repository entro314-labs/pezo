use std::path::Path;

/// File extensions that get stripped before humanizing. Anything else after a
/// dot is part of the name itself (dotted release names, versions in folder
/// names), which `file_stem` would wrongly chop.
const STRIP_EXTS: &[&str] = &[
    "exe", "msi", "zip", "7z", "rar", "tar", "gz", "bz2", "xz", "zst", "iso", "bin",
];

/// Strip split-archive suffixes: `x.part1.rar` → `x`, `x.7z.001` → `x`.
/// Byte indices are safe because to_ascii_lowercase preserves length.
fn strip_multipart_suffix(name: &str) -> Option<&str> {
    let lower = name.to_ascii_lowercase();
    if let Some(stem) = lower.strip_suffix(".rar") {
        let idx = stem.rfind(".part")?;
        let digits = &stem[idx + 5..];
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            return Some(&name[..idx]);
        }
        return None;
    }
    let (stem, digits) = lower.rsplit_once('.')?;
    let (base, ext) = stem.rsplit_once('.')?;
    if digits.len() == 3 && digits.chars().all(|c| c.is_ascii_digit()) && STRIP_EXTS.contains(&ext)
    {
        return Some(&name[..base.len()]);
    }
    None
}

/// Turn a filesystem name like `setup_broken_sword_2.0.0.6_(28495).exe`
/// into a human title like `Broken Sword`.
///
/// Title numbers survive (`Fallout 2`, `Anno 1800`, `Resident Evil 2 + 3`);
/// versions (`2.0.0.6`, `v1.02`), build ids (`28495`), bracketed tags
/// (`[FitGirl Repack]`, `(64bit)`) and release-group noise don't. Names that
/// already carry capitals keep them (`DREDGE`, `CoD`); all-lowercase names
/// are title-cased with small words left lowercase.
pub fn humanize(path: &Path) -> String {
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());

    let stem = if let Some(base) = strip_multipart_suffix(&file_name) {
        base.to_string()
    } else if path
        .extension()
        .is_some_and(|e| STRIP_EXTS.contains(&e.to_string_lossy().to_lowercase().as_str()))
    {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or(file_name)
    } else {
        file_name
    };

    let mut name = strip_bracketed(&stem);
    for prefix in ["setup_", "setup-", "install_", "game-"] {
        if name.len() >= prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix) {
            name = name[prefix.len()..].to_string();
        }
    }

    // Names with spaces are written by people: `-` and `.` are part of words
    // there (`FF-PZ`, `Mr. X`). Without spaces they're separators.
    let spaced = name.contains(' ');
    let separators: &[char] = if spaced {
        &[' ', '_']
    } else {
        &[' ', '_', '-']
    };
    let mut words: Vec<&str> = Vec::new();
    for token in name.split(separators).filter(|t| !t.is_empty()) {
        if spaced {
            if !is_version(token) {
                words.push(token);
            }
        } else {
            words.extend(dotted_words(token));
        }
    }
    words.retain(|w| !is_noise_token(w));
    // `Call of Duty Ghosts by xatab` → the group went, its `by` goes too.
    if words.last().is_some_and(|w| w.eq_ignore_ascii_case("by")) {
        words.pop();
    }

    if words.is_empty() {
        return stem;
    }
    words
        .iter()
        .enumerate()
        .map(|(i, w)| case_word(w, i == 0))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Drop `(…)` and `[…]` groups: build ids, bitness, repack tags.
fn strip_bracketed(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// `1.0`, `2.0.0.6`, `v1.02`, `1.0.2d`.
fn is_version(token: &str) -> bool {
    let t = token.strip_prefix(['v', 'V']).unwrap_or(token);
    let parts: Vec<&str> = t.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|p| is_numeric_part(p))
}

/// `12`, `2d`, `1b`: a number with at most one trailing letter.
fn is_numeric_part(p: &str) -> bool {
    let digits = p.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    !digits.is_empty() && p.len() - digits.len() <= 1 && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Split a dotted token into words, skipping version runs:
/// `Echo.Point.Nova.v1.41` → `Echo Point Nova`, `4.0` → nothing.
fn dotted_words(token: &str) -> Vec<&str> {
    let parts: Vec<&str> = token.split('.').filter(|p| !p.is_empty()).collect();
    let mut words = Vec::new();
    let mut i = 0;
    while i < parts.len() {
        let start = parts[i].strip_prefix(['v', 'V']).unwrap_or(parts[i]);
        if is_numeric_part(start) && parts.get(i + 1).is_some_and(|p| is_numeric_part(p)) {
            while i < parts.len()
                && is_numeric_part(parts[i].strip_prefix(['v', 'V']).unwrap_or(parts[i]))
            {
                i += 1;
            }
            continue;
        }
        words.push(parts[i]);
        i += 1;
    }
    words
}

fn case_word(w: &str, first: bool) -> String {
    if w.chars().any(|c| c.is_uppercase()) {
        return w.to_string();
    }
    if matches!(
        w,
        "i" | "ii" | "iii" | "iv" | "v" | "vi" | "vii" | "viii" | "ix" | "x" | "xi" | "xii"
    ) {
        return w.to_uppercase();
    }
    const SMALL: &[&str] = &[
        "a", "an", "and", "as", "at", "by", "for", "in", "of", "on", "or", "the", "to", "vs",
    ];
    if !first && SMALL.contains(&w) {
        return w.to_string();
    }
    let mut chars = w.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Derive a filesystem/db-safe slug from a title: `Broken Sword` → `broken-sword`.
pub fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut prev_dash = true;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "game".to_string()
    } else {
        trimmed
    }
}

/// Build ids, version tags, platform words and release-group names.
/// Plain title numbers (`2`, `23`, `1800`) are kept.
fn is_noise_token(w: &str) -> bool {
    if w.chars()
        .all(|c| !c.is_alphanumeric() && c != '+' && c != '&')
    {
        return true;
    }
    if w.bytes().all(|b| b.is_ascii_digit()) {
        // 1–2 digits are sequel numbers, 19xx/20xx years; anything else is a
        // build number (`28495`, `2879`).
        let year = w.len() == 4 && (w.starts_with("19") || w.starts_with("20"));
        return !(w.len() <= 2 || year);
    }
    // `v2`, `v12b`.
    if let Some(rest) = w.strip_prefix(['v', 'V'])
        && is_numeric_part(rest)
    {
        return true;
    }
    if is_version(w) {
        return true;
    }
    // Commit hashes: `d3eb3874bbc`.
    if w.len() >= 7
        && w.bytes().all(|b| b.is_ascii_hexdigit())
        && w.bytes().filter(u8::is_ascii_digit).count() >= 3
    {
        return true;
    }
    matches!(
        w.to_ascii_lowercase().as_str(),
        "gog"
            | "galaxy"
            | "offline"
            | "backup"
            | "installer"
            | "win"
            | "windows"
            | "linux"
            | "mac"
            | "macos"
            | "pc"
            | "64bit"
            | "32bit"
            | "x64"
            | "x86"
            | "repack"
            | "fitgirl"
            | "xatab"
            | "dodi"
            | "elamigos"
            | "insaneramzes"
            | "tenoke"
            | "goldberg"
            | "steamgg"
            | "ankergames"
            | "p2p"
            | "csf"
            | "plaza"
            | "codex"
            | "skidrow"
            | "reloaded"
            | "empress"
            | "fckdrm"
            | "kextraction"
            | "creamapi"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn humanizes_gog_installer_names() {
        assert_eq!(
            humanize(&PathBuf::from("setup_broken_sword_2.0.0.6_(28495).exe")),
            "Broken Sword"
        );
        assert_eq!(
            humanize(&PathBuf::from(
                "setup_heroes_of_might_and_magic_3_complete_4.0_(10665).exe"
            )),
            "Heroes of Might and Magic 3 Complete"
        );
    }

    #[test]
    fn humanizes_folders_and_archives() {
        assert_eq!(humanize(&PathBuf::from("Tyrian2000")), "Tyrian2000");
        assert_eq!(humanize(&PathBuf::from("fallout-2-v1.02.zip")), "Fallout 2");
    }

    #[test]
    fn keeps_dots_in_extensionless_folder_names() {
        assert_eq!(
            humanize(&PathBuf::from("game-project.wingman-(78697)")),
            "Project Wingman"
        );
        assert_eq!(
            humanize(&PathBuf::from(
                "game-song.of.nunu.a.league.of.legends.story-(69017)"
            )),
            "Song of Nunu a League of Legends Story"
        );
        assert_eq!(
            humanize(&PathBuf::from("Alpha_Protocol_1.1_(71214)_win_gog")),
            "Alpha Protocol"
        );
    }

    #[test]
    fn strips_multipart_suffixes() {
        assert_eq!(
            humanize(&PathBuf::from(
                "Echo.Point.Nova.v1.41-VoxanPortable.part1.rar"
            )),
            "Echo Point Nova VoxanPortable"
        );
        assert_eq!(
            humanize(&PathBuf::from("GhostExile085.7z.001")),
            "GhostExile085"
        );
        assert_eq!(humanize(&PathBuf::from("W.Skies.175.zip.001")), "W Skies");
    }

    #[test]
    fn drops_platform_tokens() {
        assert_eq!(
            humanize(&PathBuf::from("paper_trail_windows_gog_(79715)")),
            "Paper Trail"
        );
        assert_eq!(
            humanize(&PathBuf::from(
                "setup_creepy_tale_1.0.2d_(64bit)_(42424).exe"
            )),
            "Creepy Tale"
        );
    }

    #[test]
    fn keeps_roman_numerals() {
        assert_eq!(
            humanize(&PathBuf::from("setup_heroes_iii.exe")),
            "Heroes III"
        );
    }

    #[test]
    fn keeps_title_numbers_drops_build_numbers() {
        let cases = [
            ("Resident Evil 2 + 3", "Resident Evil 2 + 3"),
            ("Fable 3 [FitGirl Repack]", "Fable 3"),
            ("Luigi's Mansion 3 [FitGirl Repack]", "Luigi's Mansion 3"),
            (
                "Call of Duty - Modern Warfare 2 [FitGirl Repack]",
                "Call of Duty Modern Warfare 2",
            ),
            (
                "Call of Duty - Modern Warfare 3 [FitGirl Repack]",
                "Call of Duty Modern Warfare 3",
            ),
            ("CoD - Black Ops 3 [FitGirl Repack]", "CoD Black Ops 3"),
            ("Call of Duty  Ghosts by xatab", "Call of Duty Ghosts"),
            (
                "FF-PZ - Mask of the Lunar Eclipse [FitGirl Repack]",
                "FF-PZ Mask of the Lunar Eclipse",
            ),
            ("DREDGE_2879_(76106)_win_gog", "DREDGE"),
            (
                "Kane_and_Lynch_2_Dog_Days_1.2_(57031)_win_gog",
                "Kane and Lynch 2 Dog Days",
            ),
            ("Dino Crisis 1+2", "Dino Crisis 1+2"),
            (
                "Cthulhu.The.Cosmic.Abyss.GOG-InsaneRamZes",
                "Cthulhu The Cosmic Abyss",
            ),
            ("Aphelion.GOG-InsaneRamZes", "Aphelion"),
            (
                "CONVERGENCE_A_League_of_Legends_Story_d3eb3874bbc_(67039)_win_gog",
                "CONVERGENCE A League of Legends Story",
            ),
            (
                "legacy_of_kain_defiance_remastered_1.0.3-1239602_(64bit)_(89177)_win_gog",
                "Legacy of Kain Defiance Remastered",
            ),
            (
                "The_Legend_of_Zelda_-_Ocarina_of_Time_Repack_v2.rar",
                "The Legend of Zelda Ocarina of Time",
            ),
            ("Tyrian 2000", "Tyrian 2000"),
            ("M&M(L)", "M&M"),
            ("Tenoke.Emissary.Zero", "Emissary Zero"),
            ("Last Signal GoldBerg", "Last Signal"),
        ];
        for (input, expected) in cases {
            assert_eq!(humanize(&PathBuf::from(input)), expected, "{input}");
        }
    }

    #[test]
    fn slugifies() {
        assert_eq!(slugify("Broken Sword"), "broken-sword");
        assert_eq!(slugify("  Héroes: 3! "), "h-roes-3");
        assert_eq!(slugify("!!!"), "game");
    }
}
