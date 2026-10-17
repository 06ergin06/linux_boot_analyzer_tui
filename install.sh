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
    if [ -n "$token" ]; then
        version=$(fetch "https://api.github.com/repos/$repo/releases/latest" |
            sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)
    else
        url=$(fetch -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest")
        version="${url##*/tag/}"
    fi
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
archive_url="$base/$archive"
sums_url="$base/$archive.sha256"

# A private repository has no anonymous download URL: its assets are served by
# the API only, so their ids come out of the release JSON.
asset_url() {
    printf '%s' "$release" | awk -v want="$1" '
        match($0, /"url": *"[^"]*\/releases\/assets\/[0-9]+"/) {
            url = $0
            sub(/.*"url": *"/, "", url)
            sub(/".*/, "", url)
        }
        $0 ~ ("\"name\": *\"" want "\"") { print url; exit }
    '
}

download() {
    if [ -n "$token" ]; then
        fetch -H "Accept: application/octet-stream" -o "$2" "$1"
    else
        fetch -o "$2" "$1"
    fi
}

if [ -n "$token" ]; then
    release=$(fetch "https://api.github.com/repos/$repo/releases/tags/$tag")
    archive_url=$(asset_url "$archive")
    sums_url=$(asset_url "$archive.sha256")
    if [ -z "$archive_url" ] || [ -z "$sums_url" ]; then
        echo "release $tag does not carry $archive" >&2
        exit 1
    fi
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

echo "downloading $archive"
download "$archive_url" "$tmp/$archive"
download "$sums_url" "$tmp/$archive.sha256"

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

if as_root mkdir -p "$dir" && as_root install -m 755 "$tmp/$name-$version-$target/$name" "$dir/$name"; then
    :
elif [ "$dir" = "/usr/local/bin" ]; then
    # sudo can refuse too: no terminal to ask on, no permission, or a password
    # that is wrong. The user's own directory always works.
    echo "could not install into /usr/local/bin, using $HOME/.local/bin" >&2
    dir="$HOME/.local/bin"
    mkdir -p "$dir"
    install -m 755 "$tmp/$name-$version-$target/$name" "$dir/$name"
else
    echo "could not install into $dir" >&2
    exit 1
fi

"$dir/$name" --version
echo "installed $dir/$name"

case ":$PATH:" in
    *":$dir:"*) ;;
    *) echo "note: add $dir to PATH to run $name from anywhere" ;;
esac
