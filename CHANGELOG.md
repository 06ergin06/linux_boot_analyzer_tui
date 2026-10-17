# Changelog

Notable changes to `linux_boot_analyzer_tui`, following
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). 1.0.0 is the first
public release; development before it happened without tags.

## [1.0.0] - 2026-10-17

The first release: a terminal rewrite of the Python/GTK3
[pardus-boot-analyzer](https://github.com/06ergin06/pardus-boot-analyzer), with
the same data and the same system calls behind a keyboard-driven interface.

### Added

- **Analysis**: total boot time, the firmware/loader/kernel/initrd/userspace
  breakdown, system information, the slowest units and the enabled services that
  are safe to disable.
- **Services**: one view per unit type, search, status and suggestion filters,
  sorting, and `systemctl` enable/disable/start/stop/mask behind a confirmation
  dialog and the app's own sudo prompt.
- **Autostart**: the entries in `~/.config/autostart`, with enable/disable, a
  start delay and add/remove. Editing an entry keeps every key and comment the
  app does not own.
- **Profiles**: the three ready-made profiles with their estimated saving and
  ✔/✘ checklist, and profiles of your own — `n` creates one from the service
  list the original dialog offers, `x` deletes one, `Enter` applies it.
- **Backups**: create a restore point of every unit-file state and go back to it
  later.
- English and Turkish interface, picked from a menu with `L` or read from
  `LC_ALL`/`LANG`, stored in `~/.config/linux-boot-analyzer-tui/config.json`.
- Report export (`w`) into a text file, with the same text copied to the
  terminal clipboard through OSC 52.
- `--help` and `--version` on the command line.
- A sudo prompt that also works where the system's PAM stack loads
  `pam_fprintd`, without asking the user to change `/etc/pam.d`.

### Not ported

- "Save the current state as a profile" (`mevcut_durumu_save`): it produced the
  same plan as a restore point, only narrower, so a whole-system snapshot is the
  Backups tab's job and a profile stays a chosen set of rules.
- The GTK application's own screenshots, icons and packaging scripts; this
  project ships a plain binary.
