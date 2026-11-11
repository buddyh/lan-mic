#!/bin/bash
set -euo pipefail
app="$HOME/Applications/Lan Mic.app"
if [ ! -d "$app" ];then echo "Lan Mic app is already removed.";exit 0;fi
id="$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$app/Contents/Info.plist")"
for label in "$id" "$id.input";do launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true;rm -f "$HOME/Library/LaunchAgents/$label.plist";done
# Finder can own an agent outside launchd. Stop the exact app modes before removal
# so capture ends and the previous input is restored, even in that case.
agent_pids=""
while read -r pid command;do
 case "$command" in
  "$app/Contents/MacOS/LanMic"|"$app/Contents/MacOS/LanMic agent"|"$app/Contents/MacOS/LanMic agent --background")
   agent_pids="$agent_pids $pid";kill -TERM "$pid" 2>/dev/null || true ;;
 esac
done < <(/bin/ps -axo pid=,command=)
for pid in $agent_pids;do
 for attempt in {1..20};do if ! kill -0 "$pid" 2>/dev/null;then break;fi;/bin/sleep 0.1;done
 if kill -0 "$pid" 2>/dev/null;then kill -KILL "$pid" 2>/dev/null || true;fi
done
rm -rf "$app"
mouse="${LAN_MOUSE_BINARY:-/Applications/Lan Mouse.app/Contents/MacOS/lan-mouse}"
if [ ! -x "$mouse" ];then mouse="$HOME/Applications/Lan Mouse.app/Contents/MacOS/lan-mouse";fi
if [ -x "$mouse" ];then /usr/bin/open -na "${mouse%/Contents/MacOS/*}";fi
echo 'Removed Lan Mic. Settings/backups and BlackHole are retained; LAN Mouse is open normally.'
