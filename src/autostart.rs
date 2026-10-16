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

    for line in contents.lines() {
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
            entry.enabled = value.eq_ignore_ascii_case("true");
        } else if let Some(value) = line.strip_prefix("X-GNOME-Autostart-Delay=")
            && let Ok(delay) = value.trim().parse()
        {
            entry.delay = delay;
        }
    }

    entry
}

/// Split a `sh -c "sleep 5 && app"` command into its delay and the real command.
fn split_delay(exec: &str) -> (u32, String) {
    let original = exec.to_string();
    let Some((_, rest)) = exec.split_once("sleep") else {
        return (0, original);
    };

    let rest = rest.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let Ok(delay) = digits.parse::<u32>() else {
        return (0, original);
    };

    let after = rest[digits.len()..].trim_start();
    let Some(command) = after.strip_prefix("&&") else {
        return (0, original);
    };

    let command = command.trim().trim_matches('"').trim();
    if command.is_empty() {
        return (0, original);
    }

    (delay, command.to_string())
}

/// Render an entry as the body of a `.desktop` file.
fn render(name: &str, exec: &str, comment: &str, icon: &str, enabled: bool, delay: u32) -> String {
    let command = if delay > 0 {
        format!("sh -c \"sleep {delay} && {exec}\"")
    } else {
        exec.to_string()
    };

    let mut lines = vec![
        "[Desktop Entry]".to_string(),
        "Type=Application".to_string(),
        format!("Name={name}"),
        format!("Exec={command}"),
        format!("Comment={comment}"),
        format!("Icon={icon}"),
        format!(
            "X-GNOME-Autostart-enabled={}",
            if enabled { "true" } else { "false" }
        ),
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

fn save(entry: &AutostartEntry, enabled: bool, delay: u32) -> Result<(), String> {
    let contents = render(
        &entry.name,
        &entry.exec,
        &entry.comment,
        &entry.icon,
        enabled,
        delay,
    );
    fs::write(&entry.path, contents)
        .map_err(|err| format!("could not write {}: {err}", entry.path.display()))
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
    fn ignores_commands_without_a_delay() {
        assert_eq!(split_delay("myapp"), (0, "myapp".to_string()));
        assert_eq!(split_delay("sleepy-app"), (0, "sleepy-app".to_string()));
        assert_eq!(
            split_delay("sh -c \"sleep 5 myapp\""),
            (0, "sh -c \"sleep 5 myapp\"".to_string())
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
    fn unique_path_uses_a_slugified_name() {
        let dir = Path::new("/nonexistent-autostart-dir");
        assert_eq!(
            unique_path(dir, "My Great App"),
            dir.join("my_great_app.desktop")
        );
        assert_eq!(unique_path(dir, "  "), dir.join("autostart.desktop"));
    }
}
