#!/usr/bin/env bash
# User installation; system packages and SSH trust are configured separately.
set -euo pipefail
binary="${1:?Usage: ./install-linux.sh /path/to/lan-mic [--host SSH_HOST | --listen] [configure options]}";shift
mode=receiver
if [[ "${1:-}" == --listen ]]; then mode=agent;shift;fi
for command in pactl pacat parec systemctl ssh;do command -v "$command" >/dev/null || { echo "Missing $command. See README prerequisites." >&2;exit 1;};done
pactl info >/dev/null
create_mouse_unit=false
if ! systemctl --user cat lan-mouse.service >/dev/null 2>&1;then
 command -v lan-mouse >/dev/null || { echo 'Install and pair LAN Mouse first.' >&2;exit 1; }
 if pgrep -x lan-mouse >/dev/null;then echo 'Close LAN Mouse once, then rerun setup so its daemon can be started with handoff logging.' >&2;exit 1;fi
 create_mouse_unit=true
fi
"$binary" --version
support="${XDG_DATA_HOME:-$HOME/.local/share}/lan-mic"
state="${XDG_STATE_HOME:-$HOME/.local/state}/lan-mic"
config="${XDG_CONFIG_HOME:-$HOME/.config}"
units="$config/systemd/user"
backup="$state/backups/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$support" "$state" "$units" "$backup" "$HOME/.local/bin"
chmod 700 "$state" "$backup"
if [[ -f "$units/lan-mic.service" ]];then cp "$units/lan-mic.service" "$backup/";fi
if [[ -f "$support/lan-mic" ]];then cp "$support/lan-mic" "$backup/";fi
if [[ -f "$config/lan-mic/config.json" ]];then cp "$config/lan-mic/config.json" "$backup/";fi
systemctl --user stop lan-mic.service 2>/dev/null || true
install -m755 "$binary" "$support/lan-mic.new";mv "$support/lan-mic.new" "$support/lan-mic"
if (( $# ));then "$support/lan-mic" configure "$@";fi
if [[ "$mode" == agent ]];then "$support/lan-mic" configure --host none;fi
if [[ "$create_mouse_unit" == true ]];then
 mouse="$(command -v lan-mouse)";mouse="${mouse//%/%%}"
 capture_flag=""
 if [[ "${XDG_CURRENT_DESKTOP:-}" == *Hyprland* ]];then capture_flag="--capture-backend layer-shell";fi
 cat > "$units/lan-mouse.service" <<EOF
[Unit]
Description=LAN Mouse keyboard and mouse sharing
After=graphical-session.target
PartOf=graphical-session.target
[Service]
ExecStart="$mouse" $capture_flag daemon
Restart=on-failure
RestartSec=3
[Install]
WantedBy=graphical-session.target
EOF
 systemctl --user enable lan-mouse.service
fi
mkdir -p "$units/lan-mouse.service.d"
if [[ -f "$units/lan-mouse.service.d/lan-mic-logs.conf" ]];then cp "$units/lan-mouse.service.d/lan-mic-logs.conf" "$backup/";fi
touch "$state/lan-mouse.log";chmod 600 "$state/lan-mouse.log"
# systemd expands % inside paths. Escape literal % before writing its unit syntax.
unit_binary="${support//%/%%}/lan-mic";unit_log="${state//%/%%}/lan-mouse.log"
cat > "$units/lan-mouse.service.d/lan-mic-logs.conf" <<EOF
[Service]
StandardOutput=append:$unit_log
StandardError=append:$unit_log
EOF
cat > "$units/lan-mic.service" <<EOF
[Unit]
Description=Microphone handoff with LAN Mouse
After=graphical-session.target pipewire-pulse.service lan-mouse.service
PartOf=graphical-session.target
[Service]
ExecStart="$unit_binary" $mode
Restart=on-failure
RestartSec=3
TimeoutStopSec=15
[Install]
WantedBy=graphical-session.target
EOF
ln -sfn "$support/lan-mic" "$HOME/.local/bin/lan-mic"
systemctl --user daemon-reload
systemctl --user restart lan-mouse.service
systemctl --user enable --now lan-mic.service
printf 'Installed. Backup: %s\n' "$backup"
printf 'Status: %s receiver --status\n' "$support/lan-mic"
