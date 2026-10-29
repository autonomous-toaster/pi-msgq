#!/usr/bin/env bash
# Sleep, then wake a pi session over the message bus.
#
# Usage: notify-self.sh [seconds] [message]
# Env:   MSGQ_TARGET  session address to wake (default: session/$PI_SESSION_ID)
#        MSGQ_FROM    sender address        (default: cli/notify@<host>)
#        MSGQ_TYPE    note|prompt|steer|follow_up (default: steer)
set -euo pipefail

delay="${1:-10}"
message="${2:-your timer just fired}"
target="${MSGQ_TARGET:-session/${PI_SESSION_ID:?PI_SESSION_ID is not set}}"
from="${MSGQ_FROM:-cli/notify@$(hostname -s)}"
type="${MSGQ_TYPE:-steer}"

sleep "$delay"
msgq publish "$target" "$message" --type "$type" --from "$from"
