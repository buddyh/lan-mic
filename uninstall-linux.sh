#!/usr/bin/env bash
set -euo pipefail
config="${XDG_CONFIG_HOME:-$HOME/.config}"
support="${XDG_DATA_HOME:-$HOME/.local/share}/lan-mic"
systemctl --user disable --now lan-mic.service 2>/dev/null || true
rm -f "$config/systemd/user/lan-mic.service" "$config/systemd/user/lan-mouse.service.d/lan-mic-logs.conf"
if [[ "$(readlink "$HOME/.local/bin/lan-mic" 2>/dev/null || true)" == "$support/lan-mic" ]];then rm -f "$HOME/.local/bin/lan-mic";fi
systemctl --user daemon-reload
systemctl --user restart lan-mouse.service
printf 'Disabled Lan Mic. Your settings, audio-route state, binaries and backups are retained.\n'
printf 'The silent virtual microphone disappears at the next audio-server restart.\n'
