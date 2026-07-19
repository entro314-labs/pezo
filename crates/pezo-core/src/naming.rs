use std::path::Path;

/// Turn a filesystem name like `setup_broken_sword_2.0.0.6_(28495).exe`
/// into a human title like `Broken Sword`.
pub fn humanize(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());

    let mut s = stem.to_lowercase();
    for prefix in ["setup_", "setup-", "install_"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
        }
    }

    let cleaned: String = s
        .chars()
        .map(|c| {
            if c == '_' || c == '.' || c == '-' {
                ' '
            } else {
                c
            }
        })
        .collect();

    let words: Vec<String> = cleaned
        .split_whitespace()
        .filter(|w| !is_noise_token(w))
        .map(title_case_word)
        .collect();

    if words.is_empty() {
        stem
    } else {
        words.join(" ")
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

/// Version numbers, build ids and other tokens that don't belong in a title.
fn is_noise_token(w: &str) -> bool {
    let w = w.trim_matches(|c| c == '(' || c == ')' || c == '[' || c == ']');
    if w.is_empty() {
        return true;
    }
    // pure numbers / build ids: "28495", "1", "0"
    if w.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    // version-ish: "v1", "1a", "2b", "gog", "galaxy"
    if let Some(rest) = w.strip_prefix('v')
        && !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_digit() || c == 'a' || c == 'b')
    {
        return true;
    }
    if w.chars().next().is_some_and(|c| c.is_ascii_digit())
        && w.chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_alphabetic())
        && w.chars().filter(|c| c.is_ascii_digit()).count() >= w.len().saturating_sub(1)
    {
        return true;
    }
    matches!(w, "gog" | "galaxy" | "offline" | "backup" | "installer")
}

fn title_case_word(w: &str) -> String {
    // keep roman numerals and short acronyms uppercase
    if matches!(
        w,
        "i" | "ii" | "iii" | "iv" | "v" | "vi" | "vii" | "viii" | "ix" | "x" | "xl"
    ) {
        return w.to_uppercase();
    }
    let mut chars = w.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
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
            "Heroes Of Might And Magic Complete"
        );
    }

    #[test]
    fn humanizes_folders_and_archives() {
        assert_eq!(humanize(&PathBuf::from("Tyrian2000")), "Tyrian2000");
        assert_eq!(humanize(&PathBuf::from("fallout-2-v1.02.zip")), "Fallout");
    }

    #[test]
    fn keeps_roman_numerals() {
        assert_eq!(
            humanize(&PathBuf::from("setup_heroes_iii.exe")),
            "Heroes III"
        );
    }

    #[test]
    fn slugifies() {
        assert_eq!(slugify("Broken Sword"), "broken-sword");
        assert_eq!(slugify("  Héroes: 3! "), "h-roes-3");
        assert_eq!(slugify("!!!"), "game");
    }
}
