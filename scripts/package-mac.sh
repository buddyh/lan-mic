#!/bin/bash
# Build a clean app ZIP from source, never from the developer's installed app.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
binary="${1:?Usage: scripts/package-mac.sh /path/to/release/lan-mic [output-dir]}"
output="${2:-$root/dist}"
version="$("$binary" --version | awk '{print $2}')"
case "$(file -b "$binary")" in
 *arm64*) arch=arm64 ;;
 *x86_64*) arch=intel ;;
 *) echo 'Expected a native Mac executable.' >&2;exit 1 ;;
esac
mkdir -p "$output"
output="$(cd "$output" && pwd)"
stage="$(mktemp -d)";trap 'rm -rf "$stage"' EXIT
app="$stage/Lan Mic.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/LanMic";chmod 755 "$app/Contents/MacOS/LanMic"
plist="$app/Contents/Info.plist"
plutil -create xml1 "$plist"
for key in CFBundleName CFBundleDisplayName;do plutil -insert "$key" -string 'Lan Mic' "$plist";done
plutil -insert CFBundleIdentifier -string local.lanmic "$plist"
plutil -insert CFBundleExecutable -string LanMic "$plist"
plutil -insert CFBundlePackageType -string APPL "$plist"
plutil -insert CFBundleShortVersionString -string "$version" "$plist"
plutil -insert CFBundleVersion -string "$version" "$plist"
plutil -insert LSMinimumSystemVersion -string 14.0 "$plist"
plutil -insert NSMicrophoneUsageDescription -string 'Forward this microphone only while LAN Mouse controls your paired computer.' "$plist"
for doc in README.md README.txt VALIDATION.txt LICENSE AUTHORS THIRD_PARTY.md install-mac.sh;do cp "$root/$doc" "$app/Contents/Resources/";done
if [ -d "$root/THIRD_PARTY_LICENSES" ];then cp -R "$root/THIRD_PARTY_LICENSES" "$app/Contents/Resources/";fi
# Use an existing local release identity when explicitly selected. No keys are
# exported or stored in the repository. CI defaults to an ad-hoc preview signature.
identity="${LAN_MIC_CODESIGN_IDENTITY:--}"
if [ "$identity" = - ];then
 codesign --force --sign - --identifier local.lanmic "$app"
else
 codesign --force --options runtime --timestamp --entitlements "$root/scripts/entitlements-mac.plist" --sign "$identity" --identifier local.lanmic "$app"
fi
codesign --verify --deep --strict "$app"
if [ -n "${LAN_MIC_NOTARY_PROFILE:-}" ];then
 if [ "$identity" = - ];then echo 'Notarization requires a Developer ID identity.' >&2;exit 1;fi
 /usr/bin/ditto -c -k --keepParent "$app" "$stage/notarize.zip"
 xcrun notarytool submit "$stage/notarize.zip" --keychain-profile "$LAN_MIC_NOTARY_PROFILE" --wait
 xcrun stapler staple "$app"
 xcrun stapler validate "$app"
 spctl --assess --type execute --verbose "$app"
fi
archive="$output/lan-mic-macos-$arch.zip"
/usr/bin/ditto -c -k --keepParent "$app" "$archive"
(cd "$output" && shasum -a 256 "$(basename "$archive")" > "$(basename "$archive").sha256")
echo "$archive"
