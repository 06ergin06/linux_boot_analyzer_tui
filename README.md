# Linux Boot Analyzer TUI

[![CI](https://github.com/06ergin06/linux_boot_analyzer_tui/actions/workflows/ci.yml/badge.svg)](https://github.com/06ergin06/linux_boot_analyzer_tui/actions/workflows/ci.yml)

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
- Rust 1.88 or newer to build it. The prebuilt binaries are statically linked,
  so any x86_64 or aarch64 Linux with systemd will run them.

## Install

The install script picks the archive for your architecture, verifies its
checksum and puts the binary in `/usr/local/bin` — or in `~/.local/bin` when it
cannot write there:

```bash
curl -fsSL https://raw.githubusercontent.com/06ergin06/linux_boot_analyzer_tui/master/install.sh | sh
```

On Arch, `linux-boot-analyzer-bin` (the released binary) and
`linux-boot-analyzer-git` (built from source) are prepared under `contrib/aur/`.

Or download the archive for your architecture from the
[releases page](https://github.com/06ergin06/linux_boot_analyzer_tui/releases)
and unpack the binary somewhere on your `PATH`, or build it yourself:

```bash
cargo build --release        # ./target/release/linux-boot-analyzer
cargo install --path .       # ... or into ~/.cargo/bin
```

## Run

```bash
linux-boot-analyzer
```

## Keys

`?` shows every key, and each tab lists its own keys at the bottom of the
screen. The common ones:

| Key | Action |
| --- | --- |
| `q` / `Ctrl-C` | quit |
| `Tab` | next tab (`1`–`5` jump to one) |
| `?` | help |
| `L` | change the language |

## Your files

Everything the app writes lives in `~/.config/linux-boot-analyzer-tui/`: the
chosen language in `config.json`, restore points in `backups/`, and the profiles
you create in `profiles/`. Reports written with `w` land in the current
directory.

## License

GPL-3.0. The files in `assets/` are copied from
[pardus-boot-analyzer](https://github.com/06ergin06/pardus-boot-analyzer).
