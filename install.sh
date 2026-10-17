#!/bin/sh
# Installs the released linux-boot-analyzer binary for this architecture.
#
# Usage: sh install.sh [vX.Y.Z]
#
# Run it as a user, without sudo in front: root is asked for only when the
# binary goes into /usr/local/bin, and ~/.local/bin is used when sudo is not
# there. Set GITHUB_TOKEN (or GH_TOKEN) when the repository is private.
set -eu

repo="06ergin06/linux_boot_analyzer_tui"
name="linux-boot-analyzer"

usage() {
    cat <<'EOF'
Install the released linux-boot-analyzer binary.

Usage: sh install.sh [vX.Y.Z]

Without a version the newest release is used. The archive is checked against
its .sha256 file and the binary is installed in /usr/local/bin, or in
~/.local/bin when that cannot be written to and sudo is not available.
Set GITHUB_TOKEN (or GH_TOKEN) when the repository is private, and
LBA_INSTALL_DIR to override the destination directory.
EOF
}

version="${1:-}"
case "$version" in
    -h | --help)
        usage
        exit 0
        ;;
    v[0-9]*) ;;
    "")
        ;;
    *)
        echo "not a release version: $version (try --help)" >&2
        exit 1
        ;;
esac

token="${GITHUB_TOKEN:-${GH_TOKEN:-}}"

fetch() {
    if [ -n "$token" ]; then
        curl -fsSL -H "Authorization: Bearer $token" "$@"
    else
        curl -fsSL "$@"
    fi
}

case "$(uname -m)" in
    x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
    aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
    *)
        echo "unsupported architecture: $(uname -m); releases cover x86_64 and aarch64" >&2
        exit 1
        ;;
esac

if [ -z "$version" ]; then
    url=$(fetch -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest")
    version="${url##*/tag/}"
    case "$version" in
        v[0-9]*) ;;
        *)
            echo "no release found: is the repository private? set GITHUB_TOKEN" >&2
            exit 1
            ;;
    esac
fi

tag="$version"
version="${version#v}"

archive="$name-$version-$target.tar.gz"
base="https://github.com/$repo/releases/download/$tag"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

echo "downloading $archive"
fetch -o "$tmp/$archive" "$base/$archive"
fetch -o "$tmp/$archive.sha256" "$base/$archive.sha256"

expected=$(cut -d' ' -f1 <"$tmp/$archive.sha256")
actual=$(sha256sum "$tmp/$archive" | cut -d' ' -f1)
if [ "$expected" != "$actual" ]; then
    echo "checksum mismatch: expected $expected, got $actual" >&2
    exit 1
fi

tar -xzf "$tmp/$archive" -C "$tmp"

as_root() {
    if [ "$use_sudo" = yes ]; then
        sudo "$@"
    else
        "$@"
    fi
}

if [ -n "${LBA_INSTALL_DIR:-}" ]; then
    dir="$LBA_INSTALL_DIR"
    use_sudo=no
elif [ -w /usr/local/bin ]; then
    dir="/usr/local/bin"
    use_sudo=no
elif command -v sudo >/dev/null 2>&1; then
    dir="/usr/local/bin"
    use_sudo=yes
else
    dir="$HOME/.local/bin"
    use_sudo=no
fi

as_root mkdir -p "$dir"
as_root install -m 755 "$tmp/$name-$version-$target/$name" "$dir/$name"

"$dir/$name" --version
echo "installed $dir/$name"

case ":$PATH:" in
    *":$dir:"*) ;;
    *) echo "note: add $dir to PATH to run $name from anywhere" ;;
esac
