#!/bin/bash
# Build first, then run as the logged-in Mac user. No sudo or permission bypass.
set -euo pipefail
binary="${1:?Usage: ./install-mac.sh /absolute/path/to/lan-mic}"
source_dir="$(cd "$(dirname "$0")" && pwd)"
support="$HOME/Library/Application Support/Lan Mic"
app="$HOME/Applications/Lan Mic.app"
agents="$HOME/Library/LaunchAgents"
logs="$HOME/Library/Logs/LanMic"
domain="gui/$(id -u)"
lan_mouse="${LAN_MOUSE_BINARY:-/Applications/Lan Mouse.app/Contents/MacOS/lan-mouse}"
if [ ! -x "$lan_mouse" ]; then lan_mouse="$HOME/Applications/Lan Mouse.app/Contents/MacOS/lan-mouse"; fi
if [ ! -x "$lan_mouse" ]; then echo "Install and pair LAN Mouse before installing Lan Mic." >&2; exit 1; fi
version="$("$binary" --version | /usr/bin/awk '{print $2}')"
bundle_id="local.lanmic"
if [ -f "$app/Contents/Info.plist" ]; then bundle_id="$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$app/Contents/Info.plist")"; fi
old_id="$bundle_id"
# A downloaded app must keep its signature and stapled notarization ticket.
# Copy the complete verified bundle rather than rebuilding/re-signing its contents.
source_bundle="${binary%/Contents/MacOS/LanMic}"
prebuilt=false
if [ "$source_bundle" != "$binary" ] && [ -f "$source_bundle/Contents/Info.plist" ];then
 codesign --verify --deep --strict "$source_bundle"
 bundle_id="$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$source_bundle/Contents/Info.plist")"
 prebuilt=true
fi
input_id="$bundle_id.input"
job_labels="$old_id $old_id.input"
if [ "$old_id" != "$bundle_id" ];then job_labels="$job_labels $bundle_id $input_id";fi
backup="$support/updates/$version-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$backup" "$agents" "$logs"
chmod 700 "$support" "$backup" "$logs"
touch "$logs/helper.log" "$logs/lan-mouse.log"
chmod 600 "$logs/helper.log" "$logs/lan-mouse.log"
if [ -d "$app" ]; then /usr/bin/ditto "$app" "$backup/Lan Mic.app"; fi
if [ -f "$support/config.json" ];then cp "$support/config.json" "$backup/config.json";fi
for label in $job_labels; do
 if [ -f "$agents/$label.plist" ]; then cp "$agents/$label.plist" "$backup/"; fi
 launchctl bootout "$domain/$label" 2>/dev/null || true
done
# Finder can own the running instance if it opened during login startup.
# Stop only this app's agent modes, including an instance outside launchd.
agent_pids=""
while read -r pid command; do
 case "$command" in
  "$app/Contents/MacOS/LanMic"|"$app/Contents/MacOS/LanMic agent"|"$app/Contents/MacOS/LanMic agent --background")
   if [ "$pid" = "${LAN_MIC_SETUP_PID:-}" ];then continue;fi
   agent_pids="$agent_pids $pid";kill -TERM "$pid" 2>/dev/null || true ;;
 esac
done < <(/bin/ps -axo pid=,command=)
for pid in $agent_pids;do
 for attempt in {1..20};do if ! kill -0 "$pid" 2>/dev/null;then break;fi;/bin/sleep 0.1;done
 if kill -0 "$pid" 2>/dev/null;then kill -KILL "$pid" 2>/dev/null || true;fi
done
rollback() {
 status=$?
 if [ "$status" -ne 0 ]; then
  for label in $job_labels; do launchctl bootout "$domain/$label" 2>/dev/null || true;rm -f "$agents/$label.plist"; done
  rm -rf "$support/install-stage-$$.app"
  rm -rf "$app"
  if [ -d "$backup/Lan Mic.app" ]; then /usr/bin/ditto "$backup/Lan Mic.app" "$app"; fi
  if [ -f "$backup/config.json" ];then cp "$backup/config.json" "$support/config.json";fi
  for label in $job_labels; do
   if [ -f "$backup/$label.plist" ]; then cp "$backup/$label.plist" "$agents/"; launchctl bootstrap "$domain" "$agents/$label.plist" || true; fi
  done
 fi
 exit "$status"
}
trap rollback EXIT
load_job() {
 for attempt in {1..10};do
  if launchctl bootstrap "$domain" "$1" 2>"$backup/launchctl-error.txt";then return 0;fi
  /bin/sleep 0.2
 done
 cat "$backup/launchctl-error.txt" >&2
 return 1
}
if [ "$prebuilt" = true ];then
 if [ "$source_bundle" != "$app" ];then
  /usr/bin/ditto "$source_bundle" "$support/install-stage-$$.app"
  codesign --verify --deep --strict "$support/install-stage-$$.app"
  rm -rf "$app"
  mv "$support/install-stage-$$.app" "$app"
 fi
else
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/LanMic.new"
chmod 755 "$app/Contents/MacOS/LanMic.new"
mv "$app/Contents/MacOS/LanMic.new" "$app/Contents/MacOS/LanMic"
plist="$app/Contents/Info.plist.new"
rm -f "$plist"; plutil -create xml1 "$plist"
for key in CFBundleName CFBundleDisplayName; do plutil -insert "$key" -string 'Lan Mic' "$plist"; done
plutil -insert CFBundleIdentifier -string "$bundle_id" "$plist"
plutil -insert CFBundleExecutable -string LanMic "$plist"
plutil -insert CFBundlePackageType -string APPL "$plist"
plutil -insert CFBundleShortVersionString -string "$version" "$plist"
plutil -insert CFBundleVersion -string "$version" "$plist"
plutil -insert LSMinimumSystemVersion -string 14.0 "$plist"
plutil -insert NSMicrophoneUsageDescription -string 'Forward this microphone only while LAN Mouse controls your paired computer.' "$plist"
mv "$plist" "$app/Contents/Info.plist"
if [ "$source_dir" != "$app/Contents/Resources" ];then
 for doc in README.md README.txt VALIDATION.txt LICENSE AUTHORS THIRD_PARTY.md; do if [ -f "$source_dir/$doc" ]; then cp "$source_dir/$doc" "$app/Contents/Resources/"; fi; done
 cp "$source_dir/install-mac.sh" "$app/Contents/Resources/"
 if [ -d "$source_dir/THIRD_PARTY_LICENSES" ];then
  rm -rf "$app/Contents/Resources/THIRD_PARTY_LICENSES"
  cp -R "$source_dir/THIRD_PARTY_LICENSES" "$app/Contents/Resources/"
 fi
fi
codesign --force --sign - --identifier "$bundle_id" "$app"
fi
codesign --verify --strict "$app"
# Remove old login labels after a signed bundle changes its identifier.
if [ "$old_id" != "$bundle_id" ];then
 rm -f "$agents/$old_id.plist" "$agents/$old_id.input.plist"
fi
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$app"
# Terminate only a daemon using this exact LAN Mouse executable.
while read -r pid command; do
 if [ "$command" = "$lan_mouse daemon" ]; then kill -TERM "$pid" 2>/dev/null || true; fi
done < <(/bin/ps -axo pid=,command=)
write_job() {
 label="$1"; executable="$2"; mode="$3"; output="$4"
 plist="$agents/$label.plist.new"; rm -f "$plist"; plutil -create xml1 "$plist"
 plutil -insert Label -string "$label" "$plist"
 plutil -insert ProgramArguments -array "$plist"
 plutil -insert ProgramArguments.0 -string "$executable" "$plist"
 plutil -insert ProgramArguments.1 -string "$mode" "$plist"
 if [ "$mode" = agent ]; then plutil -insert ProgramArguments.2 -string --background "$plist"; fi
 plutil -insert RunAtLoad -bool true "$plist"
 if [ "$mode" = daemon ]; then plutil -insert KeepAlive -bool true "$plist"; else
  plutil -insert KeepAlive -dictionary "$plist"; plutil -insert KeepAlive.SuccessfulExit -bool false "$plist"
 fi
 plutil -insert ThrottleInterval -integer 5 "$plist"
 plutil -insert LimitLoadToSessionType -string Aqua "$plist"
 plutil -insert StandardOutPath -string "$output" "$plist"
 plutil -insert StandardErrorPath -string "$output" "$plist"
 chmod 600 "$plist"; mv "$plist" "$agents/$label.plist"
 load_job "$agents/$label.plist"
}
write_job "$input_id" "$lan_mouse" daemon "$logs/lan-mouse.log"
write_job "$bundle_id" "$app/Contents/MacOS/LanMic" agent "$logs/helper.log"
# Wait for the launchd instance to own the socket before opening from Finder.
for attempt in {1..30};do
 if "$app/Contents/MacOS/LanMic" ui-status >/dev/null 2>&1;then break;fi
 /bin/sleep 0.1
done
echo "Installed Lan Mic $version. Previous app/settings: $backup"
echo "Open ~/Applications/Lan Mic.app. Allow its normal microphone permission prompt."
echo "To receive audio on this Mac, install BlackHole 2ch and restart if required."
