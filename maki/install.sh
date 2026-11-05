#!/usr/bin/env bash
# Install the msgq plugin into the maki config dir.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
config="${MAKI_CONFIG_DIR:-$HOME/.config/maki}"

if [ -d "$HOME/.maki" ]; then
  echo "warning: ~/.maki exists and takes precedence over $config" >&2
fi

mkdir -p "$config/lua"
cp "$here/lua/msgq.lua" "$config/lua/msgq.lua"
echo "installed $config/lua/msgq.lua"

# An installed copy that silently lacks the locator card is the failure this
# project already hit once: the bus showed no card for any session and nothing
# said why. Check what was actually written, and the CLI it will shell out to.
if grep -q 'refresh_card' "$config/lua/msgq.lua"; then
  echo "ok: the installed plugin publishes a locator card"
else
  echo "warning: the installed plugin has no locator card code" >&2
fi

bin="${MSGQ_BIN:-msgq}"
if command -v "$bin" >/dev/null 2>&1; then
  if "$bin" card --cwd "$PWD" >/dev/null 2>&1; then
    echo "ok: $bin supports the card command"
  else
    echo "warning: $bin does not support the card command; rebuild the CLI" >&2
  fi
else
  echo "warning: $bin is not on PATH; the plugin will publish presence without a card" >&2
fi

if [ ! -f "$config/plugin.toml" ]; then
  cp "$here/plugin.toml" "$config/plugin.toml"
  echo "installed $config/plugin.toml"
else
  echo "kept existing $config/plugin.toml"
  echo "make sure it grants: run, agent, session_write, session_read, input, fs_read, fs_write, env"
fi

cat <<'EOF'

Add this to your maki init.lua (create it if needed):

  require("msgq").setup({ name = "maki" })

The session always listens on session/<session-id>; the name adds
session/<name> as an alias. Omit the name to use the id alone. Then
run /reload in maki.

Try it from a shell:

  msgq publish session/maki "say hello" --ack --await
  msgq publish session/<session-id> "say hello" --ack --await

To check where a session works from the shell, and who is on the bus:

  msgq card --cwd "$PWD" --address session/<session-id>
  msgq sessions --wide
EOF
