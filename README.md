# Linux Boot Analyzer TUI

A terminal interface for analysing and improving Linux boot time, written in
Rust with [Ratatui](https://ratatui.rs).

It is a TUI rewrite of the Python + GTK 3 application
[pardus-boot-analyzer](https://github.com/06ergin06/pardus-boot-analyzer): the
same data model, the same service knowledge base and the same behaviour,
adapted to the terminal. The interface is available in **English** and
**Turkish** and can be switched at runtime with `L`.

## Features

Five tabs, `Tab` / `Shift+Tab` or `1`–`5` to move between them.

| Tab | What it does |
| --- | --- |
| **Analysis** | Total boot time and its firmware / loader / kernel / userspace breakdown from `systemd-analyze`, system information (distribution, kernel, RAM, uptime), the slowest units from `systemd-analyze blame`, the recommended optimisations and the estimated seconds they would save. |
| **Services** | Every unit reported by `systemctl list-units --all` and `list-unit-files`, with its active / sub / boot-configuration state, its boot time and a knowledge-base tip (*critical*, *required*, *suggestion*). Three views — services, other units (socket, target, mount), devices — a search box, status / tip / sort filters and an action menu: enable, disable, start, stop, mask. |
| **Autostart** | The `~/.config/autostart/*.desktop` entries: toggle one on or off, set a start delay, add a new entry or remove the selected one. |
| **Profiles** | The three ready-made profiles — **Office Mode**, **Developer Mode** and **Minimum Services** — each with the services it would enable and disable and the estimated saving, all shown **before** anything is applied. |
| **Backups** | Restore points that store the current state of every unit, so a profile or a batch of changes can be undone in one step. |

A few details worth knowing:

- **Analysis** — `o` disables every service that the bundled data lists as safe
  to disable, that appears in the boot-time ranking and that is still enabled,
  in one confirmation; `w` writes a plain-text report to
  `boot-report-<timestamp>.txt` in the current directory and copies it to the
  system clipboard. The copy uses the OSC 52 escape sequence, so it also works
  over SSH and needs no `xclip`/`wl-copy`. Inside `tmux` the terminal only
  forwards it when `set -g set-clipboard on` is set.
- **Services** — the *boot configuration* column is the enabled state
  (`enabled`, `disabled`, `masked`, …), which is deliberately separate from the
  running state: a unit can be running right now while being switched off at
  boot. `l` shows the journal of the selected unit, `d` / `D` its dependencies
  and reverse dependencies.
- **Autostart** — the delay is written the way the desktop session understands
  it: `Exec` is wrapped in `sh -c "sleep N && …"` and
  `X-GNOME-Autostart-Delay=N` is added; enabling and disabling sets
  `X-GNOME-Autostart-enabled`.
- **Backups** — a restore point is a small JSON file holding a unit → state
  map, so restoring only touches the units that actually changed.

## Requirements

To **run** it:

- Linux with systemd; `systemctl`, `systemd-analyze` and `journalctl` on `PATH`
  (all part of systemd itself).
- `sudo` for the actions that need root.
- No extra data files: the service knowledge base and the profiles are embedded
  in the binary, so it is a single self-contained executable.

To **build** it:

- Rust 1.85 or newer (`edition = "2024"`).

## Build and run

```bash
git clone https://github.com/06ergin06/linux_boot_analyzer_tui.git
cd linux_boot_analyzer_tui

cargo run                      # debug build, straight from the source tree
cargo build --release          # optimised build
./target/release/linux_boot_analyzer_tui
```

The application starts read-only: it only reads `systemd-analyze`, `systemctl`
and `journalctl` output until you actually change something.

## Keys

`?` opens the full list inside the application, and each tab shows its own keys
in the action bar at the bottom of the screen.

| Key | Action |
| --- | --- |
| `q` / `Esc` | quit |
| `Tab` / `Shift+Tab` | next / previous tab |
| `1`–`5` | jump straight to a tab |
| `Up` / `Down`, `j` / `k` | move in a list, scroll a text page |
| `PageUp` / `PageDown` | scroll a page at a time |
| `L` | switch the language (English / Turkish) |
| `?` | help |
| `r` | reload the system data |

**Analysis**

| Key | Action |
| --- | --- |
| `o` | disable every recommended service |
| `w` | write the report and copy it to the clipboard |

**Services**

| Key | Action |
| --- | --- |
| `v` or `Left` / `Right` | unit type: services, other units, devices |
| `/` | search name and description |
| `f` / `t` / `s` | cycle the status / tip / sort filter |
| `Enter` or `a` | action menu (enable, disable, start, stop, mask) |
| `l` | journal log of the selected unit |
| `d` / `D` | dependencies / reverse dependencies |

**Autostart**

| Key | Action |
| --- | --- |
| `space` | enable or disable the entry |
| `d` | set the delay (`Up` / `Down` step one second, digits type a value) |
| `n` / `x` | add a new entry / remove the selected one |

**Profiles**

| Key | Action |
| --- | --- |
| `Enter` or `a` | apply the selected profile |

**Backups**

| Key | Action |
| --- | --- |
| `b` | create a restore point |
| `d` / `x` | restore / delete the selected backup |

## Permissions

Actions that change the system (`systemctl enable|disable|start|stop|mask`,
applying a profile, restoring a backup) are run as root by calling `systemctl`
through `sudo`. The password is typed into the application's own prompt and
handed to `sudo -S` on standard input; the terminal is never handed over to
another process, so the interface stays intact.

The application never edits system configuration files. If the distribution's
PAM stack for `sudo` loads `pam_fprintd`, the sudo child is started so that the
fingerprint module gives up immediately — the same path PAM takes for remote
sessions — and falls through to the password prompt instead of painting a
fingerprint dialog over the interface. It therefore works with the
distribution's configuration as it is, and does not need `/etc/pam.d` or
`fprintd` changed.

## Files it uses

| Path | Purpose |
| --- | --- |
| `~/.config/linux-boot-analyzer-tui/config.json` | the chosen language |
| `~/.config/linux-boot-analyzer-tui/backups/` | restore points (`backup_<timestamp>.json`) |
| `~/.config/autostart/*.desktop` | autostart entries (only what the Autostart tab edits) |
| `./boot-report-<timestamp>.txt` | the report written by `w`, in the current directory |

Nothing outside these locations is modified.

## Project layout

```
src/
  main.rs        event loop, terminal setup
  app.rs         application state, key handling, background commands
  ui.rs          rendering of every tab, overlay and bar
  system.rs      systemd-analyze / systemctl / journalctl parsers
  filters.rs     search, status / tip filters and sort order
  i18n.rs        English and Turkish strings, language detection
  service_db.rs  the embedded service knowledge base
  profiles.rs    the embedded optimisation profiles
  autostart.rs   reading and writing ~/.config/autostart
  backups.rs     creating, listing and restoring restore points
  privilege.rs   the single place that runs a command as root
  clipboard.rs   OSC 52 clipboard copy
assets/
  services_data.json   service knowledge base (verbatim from the original project)
  profiles_data.json   optimisation profiles (verbatim from the original project)
```

## Development

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The test suite is pure unit tests (no root, no systemd required). A quick
end-to-end check that the interface starts, navigates and quits cleanly:

```bash
(sleep 1; printf '2v4q') | timeout 20 script -qec \
  "stty rows 34 cols 132; ./target/debug/linux_boot_analyzer_tui" /dev/null
```

## License

[GPL-3.0](LICENSE), the same license as the original project. The service
knowledge base and the profile definitions in `assets/` are copied verbatim
from [pardus-boot-analyzer](https://github.com/06ergin06/pardus-boot-analyzer).

Developed by [İbrahim Hakkı Ergin](https://github.com/06ergin06).
