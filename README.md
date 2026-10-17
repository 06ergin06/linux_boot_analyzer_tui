# Linux Boot Analyzer TUI

A terminal UI that analyses Linux boot time. Written in Rust with
[Ratatui](https://ratatui.rs).

It is a terminal rewrite of
[pardus-boot-analyzer](https://github.com/06ergin06/pardus-boot-analyzer). The
interface is in English and Turkish.

## Features

- **Analysis** – boot time, system info, the slowest services and what can be
  optimized.
- **Services** – list systemd units, search and filter them, and enable,
  disable, start, stop or mask one.
- **Autostart** – manage the entries in `~/.config/autostart`.
- **Profiles** – apply a ready-made profile (Office, Developer, Minimum) or
  create your own.
- **Backups** – create a restore point and go back to it later.

## Requirements

- Linux with systemd.
- `sudo` for actions that need root. The password is asked inside the app.
- Rust 1.85 or newer to build it.

## Build and run

```bash
cargo run                    # debug build
cargo build --release        # optimized build
./target/release/linux_boot_analyzer_tui
```

## Keys

`?` shows every key, and each tab lists its own keys at the bottom of the
screen. The common ones:

| Key | Action |
| --- | --- |
| `q` | quit |
| `Tab` | next tab (`1`–`5` jump to one) |
| `?` | help |
| `L` | change the language |

## License

GPL-3.0. The files in `assets/` are copied from
[pardus-boot-analyzer](https://github.com/06ergin06/pardus-boot-analyzer).
