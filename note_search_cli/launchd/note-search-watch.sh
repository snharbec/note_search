#!/bin/bash
# Wrapper so launchd can pick up the same watch.env used by the systemd
# service (launchd plists have no EnvironmentFile equivalent - they can't
# source a file, so this script does it before exec'ing note_search).
set -a
[ -f "$HOME/.config/note_search/watch.env" ] && source "$HOME/.config/note_search/watch.env"
set +a

# Prefer a note_search binary next to this script (see the README's
# Troubleshooting note: if any part of $HOME - .cargo, .local, etc. - is a
# symlink onto a non-boot volume, launchd's sandbox refuses to read files
# there at all, regardless of permissions; installing script+binary
# together under e.g. ~/Library/note_search/ avoids that).
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin="$script_dir/note_search"
[ -x "$bin" ] || bin="$HOME/.cargo/bin/note_search"
[ -x "$bin" ] || bin="note_search"

exec "$bin" import --watch
