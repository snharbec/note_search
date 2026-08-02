#!/bin/bash
# Wrapper so launchd can pick up the same watch.env used by the systemd
# service (launchd plists have no EnvironmentFile equivalent - they can't
# source a file, so this script does it before exec'ing note_search).
set -a
[ -f "$HOME/.config/note_search/watch.env" ] && source "$HOME/.config/note_search/watch.env"
set +a

bin="$HOME/.cargo/bin/note_search"
[ -x "$bin" ] || bin="note_search"

exec "$bin" import --watch
