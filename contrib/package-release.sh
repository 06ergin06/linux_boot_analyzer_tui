#!/bin/sh
# Build the release archive that `.github/workflows/release.yml` publishes.
#
# Usage: contrib/package-release.sh v1.0.0
#
# It refuses a tag that does not match `Cargo.toml`, builds with the lock file,
# refuses a binary that asks for a newer glibc than the oldest system we mean to
# support, and writes `dist/linux_boot_analyzer_tui-<version>-x86_64-unknown-linux-gnu.tar.gz`
# plus its `.sha256` next to the unpacked directory.
#
# The workflow runs this on Ubuntu 22.04 on purpose: the build machine's glibc
# becomes the binary's floor. The local machine here is newer, so a local run
# needs `GLIBC_FLOOR=<that system's glibc>` to test the packaging itself.
set -eu

GLIBC_FLOOR="${GLIBC_FLOOR:-2.35}" # Ubuntu 22.04 and Debian 12, and anything newer
TARGET="x86_64-unknown-linux-gnu"
BINARY="target/release/linux_boot_analyzer_tui"

tag="${1:?usage: contrib/package-release.sh <vX.Y.Z>}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

if [ "v$version" != "$tag" ]; then
    echo "the tag $tag does not match version $version in Cargo.toml" >&2
    exit 1
fi

cargo build --release --locked

command -v objdump >/dev/null || {
    echo "objdump is needed to read the binary's glibc requirements (binutils)" >&2
    exit 1
}
required="$(objdump -T "$BINARY" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)"
[ -n "$required" ] || {
    echo "could not read the glibc requirements of $BINARY" >&2
    exit 1
}
# This machine's glibc must be the newer of the two, or the binary would not
# start on the systems the floor stands for.
if [ "$(printf '%s\n' "GLIBC_$GLIBC_FLOOR" "$required" | sort -V | tail -1)" != "GLIBC_$GLIBC_FLOOR" ]; then
    echo "the binary needs $required, newer than the GLIBC_$GLIBC_FLOOR floor" >&2
    exit 1
fi
echo "the binary needs $required, within the GLIBC_$GLIBC_FLOOR floor"

name="linux_boot_analyzer_tui-$version-$TARGET"
mkdir -p "dist/$name"
install -m 755 "$BINARY" "dist/$name/"
cp README.md LICENSE "dist/$name/"
tar -C dist -czf "dist/$name.tar.gz" "$name"
(cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")

echo "wrote dist/$name.tar.gz"
