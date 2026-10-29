#!/usr/bin/env bash
# Watch a GitLab merge request from outside pi and wake a session when something changes.
#
# The whole point is the last line of a cycle: one `msgq publish`, from an address the session
# trusts. Nothing here talks to pi, and pi does not know this script exists.
#
# Usage:
#   scripts/gitlab-watch.sh --to planner                  follow this branch's MR, forever
#   scripts/gitlab-watch.sh --to planner --once --dry-run  print the digest, send nothing
#
# Options:
#   --to <addr>        session to wake: a bare name, or a full address. Required.
#   --mr <iid>         merge request to watch. Default: the one for the current branch.
#   --interval <secs>  how often to poll. Default: 60.
#   --lines <n>        log lines to quote per failed job. Default: 40.
#   --from <addr>      sender address. Default: cli/gitlab-watch@<host>.
#   --state <dir>      where the ledger lives. Default: ~/.cache/gitlab-watch.
#   --once             one cycle, then exit. --dry-run prints instead of publishing.
#
# A branch with no open merge request is not an error. The watcher logs it once and keeps
# polling, so a branch whose MR is opened later is picked up on a following cycle.
#
# Start the session so it will *act* on the digest rather than merely show it:
#
#   pi -e ./extension --msgq-policy honor --name planner \
#      --msgq-trust "cli/gitlab-watch@$(hostname -s)"
#
# Three traps, all silent:
#   - publishing as the session itself. its own address never starts a turn.
#   - a session whose trusted set does not contain --from. the digest is shown, ignored.
#   - a session started under the default `display` policy. same result.

set -euo pipefail

SELF="cli/gitlab-watch@$(hostname -s)"
MSGQ="${MSGQ_BIN:-msgq}"
to=""
mr=""
interval=60
lines=40
once=0
dry=0
state="${XDG_CACHE_HOME:-$HOME/.cache}/gitlab-watch"

die() { printf 'gitlab-watch: %s\n' "$*" >&2; exit 1; }
log() { printf '%s gitlab-watch: %s\n' "$(date +%H:%M:%S)" "$*" >&2; }

while [ $# -gt 0 ]; do
	case "$1" in
	--to) to="${2:?--to needs an address}" && shift 2 ;;
	--mr) mr="${2:?--mr needs an iid}" && shift 2 ;;
	--interval) interval="${2:?--interval needs seconds}" && shift 2 ;;
	--lines) lines="${2:?--lines needs a count}" && shift 2 ;;
	--from) SELF="${2:?--from needs an address}" && shift 2 ;;
	--state) state="${2:?--state needs a directory}" && shift 2 ;;
	--once) once=1 && shift ;;
	--dry-run) dry=1 && shift ;;
	-h | --help)
		awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
		exit 0
		;;
	*) die "unknown argument: $1" ;;
	esac
done

[ -n "$to" ] || die "--to is required (a session name such as 'planner')"
case "$to" in
*/*) ;;
*) to="session/$to" ;;
esac

for need in jq glab; do command -v "$need" >/dev/null || die "$need is not on PATH"; done
command -v "$MSGQ" >/dev/null || die "$MSGQ is not on PATH (set MSGQ_BIN)"

# --- what we are watching ---------------------------------------------------------------

git rev-parse --git-dir >/dev/null 2>&1 || die "not in a git checkout"
branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null) || branch="HEAD"

now() { date +%H:%M; }

# Nothing about the merge request is settled here. A branch whose MR does not exist yet is a
# state to sit in, not a reason to exit, so discovery belongs to the cycle: it is retried until
# it answers, and the ledger path only exists once there is an iid to name it after.
mr_waiting=0
slug=""
seen=""
spool=""
project=""
web=""
url=""

resolve_mr() {
	if [ -n "$mr" ] && [ -n "$seen" ]; then
		return 0
	fi

	if [ -z "$mr" ]; then
		mr=$(glab mr list --source-branch "$branch" --output json 2>/dev/null |
			jq -r 'if type == "array" then (.[0].iid // empty) else (.iid // empty) end') || mr=""
		if [ -z "$mr" ]; then
			# Once, not every cycle: a line per poll would bury the events this log exists for.
			if [ "$mr_waiting" = 0 ]; then
				log "no open merge request for branch '$branch'; waiting for the next cycle"
				mr_waiting=1
			fi
			return 0
		fi
		mr_waiting=0
		log "found merge request !$mr for branch '$branch'"
	fi

	project=$(glab api "projects/:id" 2>/dev/null | jq -r '.path_with_namespace // empty') || project=""
	web=$(glab api "projects/:id" 2>/dev/null | jq -r '.web_url // empty') || web=""
	url="${web:-https://gitlab}/-/merge_requests/$mr"

	mkdir -p "$state"
	slug=$(printf '!%s' "$mr" | tr -c 'A-Za-z0-9._-' '_')
	seen="$state/$slug.seen"
	spool="$state/$slug.pending"
	touch "$seen" "$spool"

	log "watching !$mr ($branch) as $SELF -> $to every ${interval}s"
	log "ledger: $seen"
	return 0
}

# --- the ledger -------------------------------------------------------------------------
#
# Two files, because they answer two different questions.
#
#   <slug>.seen     every event this watcher has already said out loud. One key per line,
#                   greppable, and `rm` is the legitimate way to be told again.
#   <slug>.pending  events collected but not delivered, rendered as blocks. This is the
#                   replay buffer: a session that is down, restarting, or not yet started
#                   finds them on its next cycle instead of losing them.
#
# Both are named after the iid, so they are opened by resolve_mr, not here.

# --- collection -------------------------------------------------------------------------
#
# Each collector emits blocks: a `KEY <event>` line, then body lines. The first body line is
# unindented and becomes the digest's numbered heading; everything after is indented.
# A key must be stable while the event is unchanged, and different when it is new again.

collect_pipeline() {
	local pid sha jobs trace total jid name stage secs
	pid=$(glab api "projects/:id/merge_requests/$mr/pipelines?per_page=1" 2>/dev/null |
		jq -r '.[0].id // empty') || return 0
	[ -n "$pid" ] || return 0
	sha=$(glab api "projects/:id/pipelines/$pid" 2>/dev/null | jq -r '.sha // empty') || sha=""
	jobs=$(glab api "projects/:id/pipelines/$pid/jobs?per_page=100" 2>/dev/null |
		jq -r '.[] | select(.status == "failed") | [.id, .name, .stage, (.duration // 0)] | @tsv') || return 0
	[ -n "$jobs" ] || return 0

	# The sha is what re-arms the same job: a new commit is a new failure, a retry that fails
	# the same way is not.
	while IFS=$'\t' read -r jid name stage secs; do
		[ -n "$jid" ] || continue
		printf 'KEY pipeline:!%s@%s:%s\n' "$mr" "$sha" "$name"
		printf 'pipeline #%s failed: job "%s" (stage %s, %ss)\n' \
			"$pid" "$name" "$stage" "${secs%%.*}"
		printf '  full log: glab api projects/:id/jobs/%s/trace\n' "$jid"

		trace=$(mktemp)
		glab api "projects/:id/jobs/$jid/trace" 2>/dev/null >"$trace" || true
		total=$(wc -l <"$trace" | tr -d ' ')
		if [ "${total:-0}" -gt 0 ]; then
			printf '  last %s of %s lines:\n' "$lines" "$total"
			tail -n "$lines" "$trace" |
				sed $'s/\033\\[[0-9;]*m//g' |
				sed 's/^/  | /'
		else
			printf '  (the log is empty or unavailable)\n'
		fi
		rm -f "$trace"
	done <<<"$jobs"
}

collect_threads() {
	glab api "projects/:id/merge_requests/$mr/discussions?per_page=100" 2>/dev/null |
		jq -r --arg mr "$mr" '
			.[]
			| select(any(.notes[]?; .resolvable == true and .resolved == false))
			| ([.notes[]? | select(.system != true)] | last) as $n
			| select($n != null)
			| "KEY thread:!\($mr)#\(.id)@\($n.id)\n"
			  + "unresolved thread by @\($n.author.username) on "
			  + "\($n.position.new_path // $n.position.old_path // "no position"):"
			  + "\($n.position.new_line // $n.position.old_line // "?")\n"
			  + "  read: glab api projects/:id/merge_requests/\($mr)/discussions/\(.id)\n"
			  + "  answer: glab api -X POST projects/:id/merge_requests/\($mr)/discussions/\(.id)/notes -f body=\"...\"\n"
			  + ([$n.body | split("\n")[] | select(length > 0)][0:3]
			     | map("  \"" + (.[0:160]) + "\"") | join("\n"))
		' || return 0
}

# --- one cycle --------------------------------------------------------------------------

# Freshness is decided against two lists: what has been sent (the ledger), and what is already
# waiting to be sent (the spool). An event in the spool but not yet in the ledger was seen -
# by nobody, and repeating it would put the same failure in front of the reader twice.
# Only the key line is compared: a body may grow between polls (a log does) and it is still
# the same event until the key says otherwise.
fresh_of() {
	local file=$1 spoolfile=$2
	awk -v seenfile="$seen" -v spoolfile="$spoolfile" '
		BEGIN {
			while ((getline line < seenfile) > 0) seen[line] = 1
			while ((getline line < spoolfile) > 0) if (line ~ /^KEY /) seen[substr(line, 5)] = 1
			first = 1
		}
		/^KEY / { key = substr($0, 5); keep = !(key in seen)
		          if (keep) { if (!first) print ""; print; first = 0 }
		          next }
		keep { print }
	' "$file"
}

# Number the blocks as they are published, so an event keeps a position once it is in front
# of the reader even though events were collected over several cycles. The key line is the
# ledger's business, not the reader's.
number_blocks() {
	awk '/^KEY / { next }
	     { if ($0 == "") print; else if ($0 ~ /^ /) print; else { n++; printf "%d. %s\n", n, $0 } }'
}

# The session must be on the bus: a consumer with no saved offset starts at the end of the
# stream, so a record published before the session attaches is skipped as history. Presence
# is the only live signal for that, and holding costs nothing.
target_live() {
	"$MSGQ" sessions --json 2>/dev/null |
		jq -s -e --arg a "$to" 'any(.[]; .address == $a)' >/dev/null 2>&1
}

cycle() {
	local sections body combined message numbered count
	resolve_mr
	if [ -z "$mr" ]; then
		# No MR to watch: nothing to collect, nothing to publish, nothing to fail.
		return 0
	fi
	sections=$(mktemp)
	body=$(mktemp)
	combined=$(mktemp)
	message=$(mktemp)
	numbered=$(mktemp)

	{ collect_pipeline; collect_threads; } >"$sections"
	fresh_of "$sections" "$spool" >"$body"

	# What is waiting comes first: an event keeps the position it was collected at.
	{
		if [ -s "$spool" ]; then cat "$spool"; printf '\n'; fi
		cat "$body"
	} | cat -s >"$combined"

	if [ ! -s "$combined" ]; then
		rm -f "$sections" "$body" "$combined" "$message" "$numbered"
		return 0
	fi

	count=$(grep -c '^KEY ' "$combined" || true)
	number_blocks <"$combined" | cat -s >"$numbered"

	{
		printf 'gitlab: %s !%s (%s) -- %s new event(s) since %s\n' \
			"${project:-this project}" "$mr" "$branch" "$count" "$(now)"
		printf '%s\n\n' "$url"
		cat "$numbered"
	} >"$message"

	if [ "$dry" = 1 ]; then
		log "would publish to $to from $SELF (nothing written):"
		cat "$message"
		rm -f "$sections" "$body" "$combined" "$message" "$numbered"
		return 0
	fi

	if ! target_live; then
		cp "$combined" "$spool"
		log "held $count event(s): $to is not on the bus"
		rm -f "$sections" "$body" "$combined" "$message" "$numbered"
		return 0
	fi

	"$MSGQ" publish "$to" - --type prompt --from "$SELF" <"$message"
	log "published $count event(s) to $to"

	# Sent is sent: every key in the digest is remembered, including the ones that had been
	# waiting in the spool, and the spool is dropped. A digest is one message, so it is one
	# transaction: either all of it was sent or none of it was.
	grep '^KEY ' "$combined" | sed 's/^KEY //' >>"$seen"
	sort -u "$seen" -o "$seen"
	: >"$spool"
	rm -f "$sections" "$body" "$combined" "$message" "$numbered"
}

while :; do
	cycle || log "cycle failed; the ledger is unchanged"
	[ "$once" = 1 ] && break
	sleep "$interval"
done
