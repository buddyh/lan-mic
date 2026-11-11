#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
binary="${1:?Usage: scripts/package-linux.sh /path/to/release/lan-mic [output-dir]}"
output="${2:-$root/dist}"
case "$(file -b "$binary")" in
 *x86-64*) arch=x86_64 ;;
 *aarch64*) arch=arm64 ;;
 *) echo 'Expected a Linux x86_64 or arm64 executable.' >&2;exit 1 ;;
esac
"$binary" --version
mkdir -p "$output";output="$(cd "$output" && pwd)"
stage="$(mktemp -d)";trap 'rm -rf "$stage"' EXIT
dir="$stage/lan-mic-linux-$arch";mkdir "$dir"
install -m755 "$binary" "$dir/lan-mic"
for doc in README.md README.txt VALIDATION.txt LICENSE AUTHORS THIRD_PARTY.md install-linux.sh uninstall-linux.sh;do cp "$root/$doc" "$dir/";done
if [ -d "$root/THIRD_PARTY_LICENSES" ];then cp -R "$root/THIRD_PARTY_LICENSES" "$dir/";fi
archive="lan-mic-linux-$arch.tar.gz"
tar --owner=0 --group=0 --numeric-owner -C "$stage" -czf "$output/$archive" "lan-mic-linux-$arch"
(cd "$output" && sha256sum "$archive" > "$archive.sha256")
echo "$output/$archive"
