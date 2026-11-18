#!/usr/bin/env bash
# release.sh builds the release archives for the tag at HEAD:
# dist/rpt-tui_<tag>_linux_<arch>.tar.gz, each holding the static rpt-tui binary,
# and dist/SHA256SUMS. It refuses to run on a dirty tree, off a v* tag, or when the
# tag isn't v + Cargo.toml's version. Both musl targets link with Rust's own rust-lld.
set -euo pipefail
cd "$(dirname "$0")/.."

if [ -n "$(git status --porcelain)" ]; then
	echo "release.sh: the working tree has changes; commit or stash them first" >&2
	exit 1
fi
tag=$(git describe --tags --exact-match --match 'v*' HEAD 2>/dev/null) || {
	echo "release.sh: HEAD is not on a v* tag" >&2
	exit 1
}
pkgid=$(cargo pkgid)
version=${pkgid##*[#@]}
if [ "$tag" != "v$version" ]; then
	echo "release.sh: the tag $tag is not v + Cargo.toml's version ($version)" >&2
	exit 1
fi

rm -rf dist
mkdir -p dist
for arch in amd64 arm64; do
	case $arch in
	amd64) target=x86_64-unknown-linux-musl ;;
	arm64) target=aarch64-unknown-linux-musl ;;
	esac
	linker_var=CARGO_TARGET_$(echo "$target" | tr 'a-z-' 'A-Z_')_LINKER
	env "$linker_var=rust-lld" cargo build --release --locked --target "$target"
	dir=$(mktemp -d)
	cp "target/$target/release/rpt-tui" "$dir/rpt-tui"
	tar -C "$dir" --owner=0 --group=0 --numeric-owner -czf "dist/rpt-tui_${tag}_linux_${arch}.tar.gz" rpt-tui
	rm -rf "$dir"
done
(cd dist && sha256sum ./*.tar.gz | sed 's# \./# #' > SHA256SUMS)
echo "release.sh: built $tag:"
ls -1 dist
