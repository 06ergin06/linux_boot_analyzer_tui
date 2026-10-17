#!/bin/sh
# Usage: contrib/package-release.sh <vX.Y.Z> [target]
#
# Builds the static archive `release.yml` publishes, checking the tag against
# `Cargo.toml` and the binary for static linking. The target defaults to
# `x86_64-unknown-linux-musl` and needs `musl-tools` for its `musl-gcc`.
set -eu

tag="${1:?usage: contrib/package-release.sh <vX.Y.Z> [target]}"
TARGET="${2:-x86_64-unknown-linux-musl}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

if [ "v$version" != "$tag" ]; then
    echo "the tag $tag does not match version $version in Cargo.toml" >&2
    exit 1
fi

cargo build --release --locked --target "$TARGET"

binary="target/$TARGET/release/linux_boot_analyzer_tui"
[ -f "$binary" ] || {
    echo "no binary at $binary" >&2
    exit 1
}
command -v readelf >/dev/null || {
    echo "readelf is needed to check that the binary is static (binutils)" >&2
    exit 1
}
# No program interpreter means the libc of the machine that runs it cannot
# matter; a dynamic binary would refuse to start on distributions older than
# the machine that built it.
if readelf -l "$binary" | grep -q 'INTERP'; then
    echo "$binary is dynamically linked; check the target and its linker" >&2
    exit 1
fi
echo "$binary is statically linked"

name="linux_boot_analyzer_tui-$version-$TARGET"
mkdir -p "dist/$name"
install -m 755 "$binary" "dist/$name/"
cp README.md LICENSE "dist/$name/"
tar -C dist -czf "dist/$name.tar.gz" "$name"
(cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")

echo "wrote dist/$name.tar.gz"
