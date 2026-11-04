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
EOF
