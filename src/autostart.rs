//! Reads and edits `~/.config/autostart/*.desktop` entries.
//!
//! Ported from the Python project's `autostart_manager.py`.

use std::fs;
use std::path::{Path, PathBuf};

/// One autostart entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutostartEntry {
    pub path: PathBuf,
    pub name: String,
    pub exec: String,
    pub comment: String,
    pub icon: String,
    /// Seconds to wait before launching, from the `sleep` wrapper/delay key.
    pub delay: u32,
    pub enabled: bool,
}

/// The user's autostart directory.
pub fn dir() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/autostart")
}

/// Every `.desktop` entry in the autostart directory.
pub fn entries() -> Result<Vec<AutostartEntry>, String> {
    let dir = dir();
    let listing = match fs::read_dir(&dir) {
        Ok(listing) => listing,
        // A missing directory just means there is nothing to autostart.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("could not read {}: {err}", dir.display())),
    };

    let mut entries = Vec::new();
    for item in listing.flatten() {
        let path = item.path();
        let is_desktop = path
            .extension()
            .is_some_and(|extension| extension == "desktop");
        if is_desktop && let Ok(contents) = fs::read_to_string(&path) {
            entries.push(parse(&path, &contents));
        }
    }

    entries.sort_by_key(|entry| entry.name.to_lowercase());
    Ok(entries)
}

/// Parse one `.desktop` file.
pub fn parse(path: &Path, contents: &str) -> AutostartEntry {
    let mut entry = AutostartEntry {
        path: path.to_path_buf(),
        name: String::new(),
        exec: String::new(),
        comment: String::new(),
        icon: "system-run".to_string(),
        delay: 0,
        enabled: true,
    };

    // The two "do not start me" keys are independent: the entry is enabled
    // only when neither of them says otherwise.
    let mut gnome_enabled = true;
    let mut hidden = false;

    for line in entry_lines(contents) {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Name=") {
            entry.name = value.to_string();
        } else if let Some(value) = line.strip_prefix("Exec=") {
            let (delay, command) = split_delay(value);
            entry.exec = command;
            if delay > 0 {
                entry.delay = delay;
            }
        } else if let Some(value) = line.strip_prefix("Comment=") {
            entry.comment = value.to_string();
        } else if let Some(value) = line.strip_prefix("Icon=") {
            entry.icon = value.to_string();
        } else if let Some(value) = line.strip_prefix("X-GNOME-Autostart-enabled=") {
            gnome_enabled = value.trim().eq_ignore_ascii_case("true");
        } else if let Some(value) = line.strip_prefix("Hidden=") {
            hidden = value.trim().eq_ignore_ascii_case("true");
        } else if let Some(value) = line.strip_prefix("X-GNOME-Autostart-Delay=")
            && let Ok(delay) = value.trim().parse()
        {
            entry.delay = delay;
        }
    }

    entry.enabled = gnome_enabled && !hidden;
    entry
}

/// The lines of the `[Desktop Entry]` group.
///
/// A desktop file may hold other groups (`[Desktop Action ...]`) with keys of
/// the same name; only the entry group describes the autostart entry itself.
/// Lines before the first group header count as part of it.
fn entry_lines(contents: &str) -> impl Iterator<Item = &str> {
    let mut in_entry = true;
    contents.lines().filter(move |line| {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line.starts_with("[Desktop Entry]");
            return false;
        }
        in_entry
    })
}

/// Split a `sh -c "sleep 5 && app"` wrapper into its delay and the command.
///
/// Both the double-quoted form the original application writes and the
/// single-quoted form this module writes are recognised; anything else is
/// returned unchanged with a delay of zero.
fn split_delay(exec: &str) -> (u32, String) {
    let original = exec.to_string();
    let Some((body, single_quoted)) = sh_c_body(exec) else {
        return (0, original);
    };
    let Some(rest) = body.strip_prefix("sleep ") else {
        return (0, original);
    };

    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let Ok(delay) = digits.parse::<u32>() else {
        return (0, original);
    };

    let after = rest[digits.len()..].trim_start();
    let Some(command) = after.strip_prefix("&&") else {
        return (0, original);
    };

    let command = command.trim();
    if command.is_empty() {
        return (0, original);
    }

    (delay, unescape(command, single_quoted))
}

/// The body of a `sh -c "..."` (or `sh -c '...'`) command, and the quote used.
fn sh_c_body(exec: &str) -> Option<(&str, bool)> {
    let rest = exec.trim().strip_prefix("sh -c ")?;
    let rest = rest.trim();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }

    let body = rest.strip_prefix(quote)?.strip_suffix(quote)?;
    Some((body, quote == '\''))
}

/// Undo the quoting of a command read out of a wrapper.
fn unescape(command: &str, single_quoted: bool) -> String {
    if single_quoted {
        // Inside single quotes every character is literal; the quote itself is
        // written as `'\''`.
        return command.replace("'\\''", "'");
    }

    // The double-quoted form only escapes these two characters.
    let mut out = String::with_capacity(command.len());
    let mut chars = command.chars();
    while let Some(current) = chars.next() {
        if current != '\\' {
            out.push(current);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The `Exec=` value: the command, wrapped in a shell that waits first.
fn exec_line(command: &str, delay: u32) -> String {
    if delay == 0 || command.trim().is_empty() {
        return command.to_string();
    }

    // A single-quoted body needs no escaping except for the quote itself.
    let quoted = command.replace('\'', "'\\''");
    format!("sh -c 'sleep {delay} && {quoted}'")
}

/// GNOME's text for a boolean key.
fn bool_text(value: bool) -> String {
    if value { "true" } else { "false" }.to_string()
}

/// Render an entry as the body of a new `.desktop` file.
fn render(name: &str, exec: &str, comment: &str, icon: &str, enabled: bool, delay: u32) -> String {
    let mut lines = vec![
        "[Desktop Entry]".to_string(),
        "Type=Application".to_string(),
        format!("Name={name}"),
        format!("Exec={}", exec_line(exec, delay)),
        format!("Comment={comment}"),
        format!("Icon={icon}"),
        format!("X-GNOME-Autostart-enabled={}", bool_text(enabled)),
    ];
    if delay > 0 {
        lines.push(format!("X-GNOME-Autostart-Delay={delay}"));
    }

    lines.join("\n") + "\n"
}

/// Create a new autostart entry and return the file written.
pub fn add(name: &str, command: &str) -> Result<PathBuf, String> {
    let dir = dir();
    fs::create_dir_all(&dir).map_err(|err| format!("could not create {}: {err}", dir.display()))?;

    let path = unique_path(&dir, name);
    let contents = render(name, command, "", "system-run", true, 0);
    fs::write(&path, contents)
        .map_err(|err| format!("could not write {}: {err}", path.display()))?;

    Ok(path)
}

/// Remove an entry.
pub fn remove(path: &Path) -> Result<(), String> {
    fs::remove_file(path).map_err(|err| format!("could not remove {}: {err}", path.display()))
}

/// Rewrite an entry with a new enabled state.
pub fn set_enabled(entry: &AutostartEntry, enabled: bool) -> Result<(), String> {
    save(entry, enabled, entry.delay)
}

/// Rewrite an entry with a new delay.
pub fn set_delay(entry: &AutostartEntry, delay: u32) -> Result<(), String> {
    save(entry, entry.enabled, delay)
}

/// Save `enabled` and `delay` into the entry's file.
///
/// The file is read back and only the keys this module owns are replaced, so
/// a setting another tool wrote (`OnlyShowIn`, `Name[tr]`, a desktop action,
/// ...) survives the edit.
fn save(entry: &AutostartEntry, enabled: bool, delay: u32) -> Result<(), String> {
    let contents = fs::read_to_string(&entry.path)
        .map_err(|err| format!("could not read {}: {err}", entry.path.display()))?;
    let updated = update(&contents, entry, enabled, delay);
    fs::write(&entry.path, updated)
        .map_err(|err| format!("could not write {}: {err}", entry.path.display()))
}

/// `contents` with the owned keys set to the wanted values.
fn update(contents: &str, entry: &AutostartEntry, enabled: bool, delay: u32) -> String {
    let fields: [(&str, String); 4] = [
        ("Exec", exec_line(&entry.exec, delay)),
        ("X-GNOME-Autostart-enabled", bool_text(enabled)),
        ("X-GNOME-Autostart-Delay", delay.to_string()),
        // Keeps the two "do not start me" keys saying the same thing, but only
        // when the file already uses this one; it is never added.
        ("Hidden", bool_text(!enabled)),
    ];

    let mut in_entry = true;
    let mut seen = [false; 4];
    let mut lines: Vec<String> = Vec::new();

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_entry = trimmed.starts_with("[Desktop Entry]");
            lines.push(line.to_string());
            continue;
        }

        let index = in_entry
            .then(|| trimmed.split_once('=').map(|(key, _)| key.trim()))
            .flatten()
            .and_then(|key| fields.iter().position(|(name, _)| *name == key));

        match index {
            Some(index) => {
                seen[index] = true;
                lines.push(format!("{}={}", fields[index].0, fields[index].1));
            }
            None => lines.push(line.to_string()),
        }
    }

    // A key the file did not have is only added when it carries information:
    // the command itself, the enabled flag, and a delay worth waiting for.
    let missing: Vec<String> = fields
        .iter()
        .enumerate()
        .filter(|(index, (name, _))| {
            !seen[*index]
                && *name != "Hidden"
                && !(*name == "X-GNOME-Autostart-Delay" && delay == 0)
        })
        .map(|(_, (name, value))| format!("{name}={value}"))
        .collect();

    if !missing.is_empty() {
        // Somewhere inside the entry group, not after a later group.
        let at = lines
            .iter()
            .position(|line| line.trim() == "[Desktop Entry]")
            .map_or(lines.len(), |index| index + 1);
        lines.splice(at..at, missing);
    }

    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// A filename derived from `name` that does not clash with an existing file.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let slug: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-'))
        .collect();
    let slug = slug.trim().replace(' ', "_").to_lowercase();
    let slug = if slug.is_empty() {
        "autostart".to_string()
    } else {
        slug
    };

    let mut path = dir.join(format!("{slug}.desktop"));
    let mut counter = 1;
    while path.exists() {
        path = dir.join(format!("{slug}_{counter}.desktop"));
        counter += 1;
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write `contents` to a temporary file and return its path.
    fn temp_entry(case: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("opencode-autostart-{case}.desktop"));
        fs::write(&path, contents).expect("temporary file must be writable");
        path
    }

    #[test]
    fn parses_a_desktop_entry() {
        let entry = parse(
            Path::new("/tmp/example.desktop"),
            "[Desktop Entry]\nType=Application\nName=My App\nExec=myapp --flag\n\
             Comment=Test\nIcon=my-icon\nX-GNOME-Autostart-enabled=false\n",
        );

        assert_eq!(entry.name, "My App");
        assert_eq!(entry.exec, "myapp --flag");
        assert_eq!(entry.comment, "Test");
        assert_eq!(entry.icon, "my-icon");
        assert!(!entry.enabled);
        assert_eq!(entry.delay, 0);
    }

    #[test]
    fn extracts_a_sleep_delay() {
        let entry = parse(
            Path::new("/tmp/example.desktop"),
            "Name=Delayed\nExec=sh -c \"sleep 15 && myapp\"\n",
        );
        assert_eq!(entry.delay, 15);
        assert_eq!(entry.exec, "myapp");
    }

    #[test]
    fn a_hidden_entry_counts_as_disabled() {
        let entry = parse(
            Path::new("/tmp/example.desktop"),
            "Name=Hidden\nExec=myapp\nHidden=true\n",
        );
        assert!(!entry.enabled);
    }

    #[test]
    fn ignores_commands_without_a_delay() {
        assert_eq!(split_delay("myapp"), (0, "myapp".to_string()));
        assert_eq!(split_delay("sleepy-app"), (0, "sleepy-app".to_string()));
        assert_eq!(
            split_delay("sh -c \"sleep 5 myapp\""),
            (0, "sh -c \"sleep 5 myapp\"".to_string())
        );
    }

    #[test]
    fn keeps_a_command_that_holds_a_quote() {
        let wrapped = exec_line("notify-send 'hi there'", 5);
        assert_eq!(wrapped, "sh -c 'sleep 5 && notify-send '\\''hi there'\\'''");
        assert_eq!(
            split_delay(&wrapped),
            (5, "notify-send 'hi there'".to_string())
        );
    }

    #[test]
    fn render_round_trips_through_parse() {
        let contents = render("My App", "myapp --flag", "A comment", "my-icon", false, 7);
        let entry = parse(Path::new("/tmp/roundtrip.desktop"), &contents);

        assert_eq!(entry.name, "My App");
        assert_eq!(entry.exec, "myapp --flag");
        assert_eq!(entry.comment, "A comment");
        assert_eq!(entry.icon, "my-icon");
        assert!(!entry.enabled);
        assert_eq!(entry.delay, 7);
    }

    #[test]
    fn without_delay_no_wrapper_or_delay_key_is_written() {
        let contents = render("Plain", "plainapp", "", "system-run", true, 0);
        assert!(contents.contains("Exec=plainapp\n"));
        assert!(!contents.contains("X-GNOME-Autostart-Delay"));
    }

    #[test]
    fn saving_keeps_the_keys_it_does_not_own() {
        let path = temp_entry(
            "keep-keys",
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=App\n\
             Name[tr]=Uygulama\n\
             Exec=app\n\
             Icon=app\n\
             OnlyShowIn=GNOME;\n\
             X-GNOME-Autostart-enabled=true\n\
             \n\
             [Desktop Action extra]\n\
             Name=Extra\n\
             Exec=other-action\n",
        );
        let entry = parse(&path, &fs::read_to_string(&path).expect("read"));
        set_delay(&entry, 4).expect("save must succeed");

        let saved = fs::read_to_string(&path).expect("read");
        let _ = fs::remove_file(&path);

        assert!(saved.contains("Name[tr]=Uygulama\n"), "{saved}");
        assert!(saved.contains("OnlyShowIn=GNOME;\n"), "{saved}");
        assert!(saved.contains("Exec=sh -c 'sleep 4 && app'\n"), "{saved}");
        assert!(saved.contains("X-GNOME-Autostart-Delay=4\n"), "{saved}");
        // The action's own command is in another group and stays untouched.
        assert!(saved.contains("Exec=other-action\n"), "{saved}");
        assert!(saved.contains("[Desktop Action extra]\n"), "{saved}");
    }

    #[test]
    fn enabling_clears_a_hidden_flag() {
        let path = temp_entry(
            "hidden",
            "[Desktop Entry]\nName=App\nExec=app\nHidden=true\n",
        );
        let entry = parse(&path, &fs::read_to_string(&path).expect("read"));
        assert!(!entry.enabled);

        set_enabled(&entry, true).expect("save must succeed");
        let saved = fs::read_to_string(&path).expect("read");
        let _ = fs::remove_file(&path);

        assert!(saved.contains("Hidden=false\n"), "{saved}");
        assert!(
            saved.contains("X-GNOME-Autostart-enabled=true\n"),
            "{saved}"
        );
        assert!(parse(&path, &saved).enabled);
    }

    #[test]
    fn disabling_writes_the_enabled_key() {
        let path = temp_entry("disable", "[Desktop Entry]\nName=App\nExec=app\n");
        let entry = parse(&path, &fs::read_to_string(&path).expect("read"));

        set_enabled(&entry, false).expect("save must succeed");
        let saved = fs::read_to_string(&path).expect("read");
        let _ = fs::remove_file(&path);

        assert!(
            saved.contains("X-GNOME-Autostart-enabled=false\n"),
            "{saved}"
        );
        // The file never used the other key, so it is not invented.
        assert!(!saved.contains("Hidden="), "{saved}");
        assert!(!parse(&path, &saved).enabled);
    }

    #[test]
    fn a_missing_key_is_added_inside_the_entry_group() {
        let path = temp_entry(
            "add-key",
            "[Desktop Entry]\nName=App\nExec=app\n\n[Desktop Action extra]\nExec=other\n",
        );
        let entry = parse(&path, &fs::read_to_string(&path).expect("read"));

        set_delay(&entry, 3).expect("save must succeed");
        let saved = fs::read_to_string(&path).expect("read");
        let _ = fs::remove_file(&path);

        let added = saved.find("X-GNOME-Autostart-Delay=3").expect("delay key");
        let action = saved.find("[Desktop Action extra]").expect("action group");
        assert!(added < action, "{saved}");
        assert!(saved.contains("Exec=sh -c 'sleep 3 && app'\n"), "{saved}");
        assert!(saved.ends_with("Exec=other\n"), "{saved}");
    }

    #[test]
    fn removing_a_delay_unwraps_the_command() {
        let path = temp_entry(
            "unwrap",
            "[Desktop Entry]\nName=App\nExec=app\nX-GNOME-Autostart-Delay=9\n",
        );
        let entry = parse(&path, &fs::read_to_string(&path).expect("read"));
        assert_eq!(entry.delay, 9);

        set_delay(&entry, 0).expect("save must succeed");
        let saved = fs::read_to_string(&path).expect("read");
        let _ = fs::remove_file(&path);

        assert!(saved.contains("Exec=app\n"), "{saved}");
        assert!(saved.contains("X-GNOME-Autostart-Delay=0\n"), "{saved}");
    }

    #[test]
    fn unique_path_uses_a_slugified_name() {
        let dir = Path::new("/nonexistent-autostart-dir");
        assert_eq!(
            unique_path(dir, "My Great App"),
            dir.join("my_great_app.desktop")
        );
        assert_eq!(unique_path(dir, "  "), dir.join("autostart.desktop"));
    }
}
