-- msgq: steer this maki session from the command line.
--
-- Consumes `session/<name>` (or session/<session-id>) with the msgq CLI and
-- delivers records into the focused session. Also gives the agent a `msgq`
-- tool to send, reply and list peers, and the human `/peers`, `/peer` and
-- `/reply` commands.
--
-- Configure from init.lua:
--
--   require("msgq").setup({ name = "planner" })
--
-- The module schedules its own start just after the config dir finishes
-- loading, so setup() has always run by then.

local M = {}

local FOLLOW_JOB_PREFIX = "msgq-follow-"
local PRESENCE_ADDRESS = "group/presence"
local DEFAULT_TTL = 30
local DEFAULT_EVERY_MS = 10000
local RESTART_MS = 2000
local MAX_RESTART_MS = 5000
local RUN_TIMEOUT_MS = 15000

local CONFIG = {
	name = nil,
	policy = "honor", -- "honor" | "display"
	trust = nil, -- nil = every sender, or a list of addresses
	presence = true,
	bin = nil, -- nil = $MSGQ_BIN or "msgq"
	db = nil, -- nil = $MSGQ_DB or the CLI default
	topics = nil, -- extra addresses to follow
	ttl = DEFAULT_TTL,
	presence_every_ms = DEFAULT_EVERY_MS,
	context = "auto", -- "auto" | "off"
	card_paths = "relative", -- "relative" | "full" | "none"
}

local S = {
	started = false,
	start_scheduled = false,
	stopping = false,
	session_id = nil,
	address = nil,
	addresses = {},
	consumer = nil,
	follows = {},
	follow_job = nil,
	presence_task = nil,
	presence_state = "idle",
	derived_card = nil,
	card = nil,
	card_overrides = {},
	restart_ms = RESTART_MS,
	last_error = nil,
	last_inbound = nil, -- { from, offset, id, message }
	pending = {}, -- ack state, keyed by offset
	autocmds = {},
}

-- ---------------------------------------------------------------------------
-- Small utilities
-- ---------------------------------------------------------------------------

local function trim(s)
	return (s:gsub("^%s+", ""):gsub("%s+$", ""))
end

local function log(level, msg)
	local line = "[msgq] " .. msg
	local fn = maki.log[level] or maki.log.info
	pcall(fn, line)
end

local function fail(msg)
	S.last_error = msg
	log("warn", msg)
	maki.notify(msg, "warn")
end

local function env(name)
	local ok, value = pcall(function()
		return maki.uv.os_getenv(name)
	end)
	if ok and type(value) == "string" and value ~= "" then
		return value
	end
	return nil
end

local function bin()
	return CONFIG.bin or env("MSGQ_BIN") or "msgq"
end

local function db_path()
	return CONFIG.db or env("MSGQ_DB")
end

-- Prepend the database override, then the command's own arguments.
local function base(args)
	local argv = { bin() }
	local db = db_path()
	if db then
		argv[#argv + 1] = "--db"
		argv[#argv + 1] = db
	end
	for _, a in ipairs(args) do
		argv[#argv + 1] = a
	end
	return argv
end

-- Run msgq and wait for it. Returns stdout, or nil plus an error.
local function run(args, timeout_ms)
	local id, err = maki.fn.jobstart(base(args))
	if not id then
		return nil, tostring(err or "could not start msgq")
	end
	local res, wait_err = maki.fn.jobwait(id, timeout_ms or RUN_TIMEOUT_MS)
	if not res then
		return nil, tostring(wait_err or "msgq did not finish")
	end
	if res.exit_code ~= 0 then
		local text = res.stderr
		if not text or text == "" then
			text = res.stdout
		end
		text = trim(text or "")
		if text == "" then
			text = "msgq exited " .. tostring(res.exit_code)
		end
		return nil, text
	end
	return trim(res.stdout or "")
end

-- Run msgq without waiting, owned by this session so it survives a /reload.
local function fire(args)
	local id
	id = maki.fn.jobstart(base(args), {
		scope = { session = S.session_id },
		stdout = false,
		stderr = false,
		on_exit = function()
			if id then
				pcall(maki.fn.jobforget, id)
			end
		end,
	})
	if not id then
		S.last_error = "could not run msgq"
	end
	return id
end

local function has_ui()
	local st = maki.ui.input()
	return st ~= nil
end

-- ---------------------------------------------------------------------------
-- Bus helpers
-- ---------------------------------------------------------------------------

local function publish_now(to, body, opts)
	opts = opts or {}
	local args = { "publish", to, "--from", S.address }
	if opts.type then
		args[#args + 1] = "--type"
		args[#args + 1] = opts.type
	end
	if opts.reply_to then
		args[#args + 1] = "--reply-to"
		args[#args + 1] = opts.reply_to
	end
	if opts.ack then
		args[#args + 1] = "--ack"
		args[#args + 1] = opts.ack
	end
	args[#args + 1] = "--"
	args[#args + 1] = body
	return run(args)
end

local function list_peers()
	local out, err = run({ "sessions", "--json" }, 8000)
	if not out then
		return nil, err
	end
	local entries = {}
	for line in out:gmatch("[^\n]+") do
		local ok, value = pcall(maki.json.decode, line)
		if ok and type(value) == "table" and type(value.address) == "string" then
			entries[#entries + 1] = value
		end
	end
	return entries
end

local function others(entries)
	local out = {}
	for _, entry in ipairs(entries or {}) do
		if entry.address ~= S.address then
			out[#out + 1] = entry
		end
	end
	return out
end

local function card_summary(entry)
	local card = entry.card
	if type(card) ~= "table" then
		return ""
	end
	local parts = {}
	for _, key in ipairs({ "project", "branch", "path" }) do
		local value = card[key]
		if type(value) == "string" and value ~= "" and not (key == "path" and value == ".") then
			parts[#parts + 1] = value
		end
	end
	return table.concat(parts, "  ")
end

local function roster_tail(entry)
	local state = entry.state or "-"
	local tail = state
	if entry.expires_in ~= nil then
		tail = tail .. "  " .. tostring(entry.expires_in) .. "s"
	end
	local summary = card_summary(entry)
	if summary ~= "" then
		tail = tail .. "  " .. summary
	end
	return tail
end

local function format_roster(entries)
	local lines = {}
	for _, entry in ipairs(entries) do
		lines[#lines + 1] = "  " .. entry.address .. "  " .. roster_tail(entry)
	end
	if #lines == 0 then
		lines[1] = "  no other participant is reachable on the bus"
	end
	return lines
end

-- `me` is this session, a bare name is a session, anything with a scheme is
-- passed through and validated by msgq itself. Used by the human commands.
local function resolve_address(raw)
	local value = trim(raw or "")
	if value == "" then
		return nil, "no address given"
	end
	if value == "me" then
		if not S.address then
			return nil, "this session has no bus address yet"
		end
		return S.address
	end
	if value:find("/", 1, true) then
		return value
	end
	return "session/" .. value
end

-- The agent's addresses follow the CLI's grammar: a bare name is a group.
local function resolve_bus_address(raw)
	local value = trim(raw or "")
	if value == "" then
		return nil, "no address given"
	end
	if value == "me" then
		if not S.address then
			return nil, "this session has no bus address yet"
		end
		return S.address
	end
	return value
end

local function normalize_topic(value)
	if value:find("/", 1, true) then
		return value
	end
	return "group/" .. value
end

-- ---------------------------------------------------------------------------
-- UI helpers
-- ---------------------------------------------------------------------------

local function pick_peer(entries, title)
	if #entries == 0 then
		maki.notify("no other participant is reachable on the bus", "warn")
		return nil
	end
	if not has_ui() then
		maki.notify("peers: " .. table.concat(format_roster(entries), "; "))
		return nil
	end
	local ok, ListPicker = pcall(require, "maki.list_picker")
	if not ok then
		fail("maki.list_picker is unavailable: " .. tostring(ListPicker))
		return nil
	end
	local items = {}
	for _, entry in ipairs(entries) do
		items[#items + 1] = { label = entry.address, detail = roster_tail(entry) }
	end
	local result = ListPicker.open(items, {
		title = title or "Send to a peer",
		footer = { { "q", "close" }, { "<CR>", "select" } },
	})
	if result and result.type == "choice" and result.item then
		return result.item.label
	end
	return nil
end

local function show_text(title, lines)
	if not has_ui() then
		maki.notify(title .. ": " .. table.concat(lines, "; "))
		return
	end
	local buf = maki.ui.buf()
	for _, line in ipairs(lines) do
		buf:line(line)
	end
	local height = math.min(#lines + 2, 22)
	local win = maki.ui.open_win(buf, {
		title = title,
		width = "70%",
		height = height,
		footer = { { "q", "close" }, { "<Esc>", "close" } },
	})
	while win do
		local ev = win:recv()
		if not ev then
			break
		end
		if ev.type == "close" then
			break
		end
		if ev.type == "key" and (ev.key == "q" or ev.key == "<Esc>") then
			break
		end
	end
	if win then
		pcall(function()
			win:close()
		end)
	end
end

local function draft_path()
	local dir = maki.env.state_dir() or "/tmp"
	return dir .. "/msgq-draft.md"
end

local function compose()
	if not has_ui() then
		maki.notify("composing a message needs an interactive session", "warn")
		return nil
	end
	local path = draft_path()
	local ok, err = maki.fs.write(path, "")
	if not ok then
		fail("could not create the draft: " .. tostring(err))
		return nil
	end
	local code = maki.ui.open_editor(path)
	if code ~= 0 then
		pcall(maki.fs.rm, path, { force = true })
		if code ~= -1 then
			maki.notify("editor exited with " .. tostring(code), "warn")
		end
		return nil
	end
	local text = maki.fs.read(path)
	pcall(maki.fs.rm, path, { force = true })
	if type(text) ~= "string" then
		return nil
	end
	text = trim(text)
	if text == "" then
		maki.notify("nothing sent")
		return nil
	end
	return text
end

-- ---------------------------------------------------------------------------
-- Delivery
-- ---------------------------------------------------------------------------

local function trusted(from)
	if CONFIG.trust == nil then
		return true
	end
	for _, allowed in ipairs(CONFIG.trust) do
		if allowed == "*" or allowed == from then
			return true
		end
	end
	return false
end

local function wants_ack(record)
	local meta = record.meta
	if type(meta) ~= "table" then
		return nil
	end
	local ack = meta.ack
	if ack == nil or ack == false then
		return nil
	end
	if ack == true then
		return { received = true, started = true, completed = true }
	end
	if type(ack) == "string" then
		local phases = {}
		for phase in ack:gmatch("[^,]+") do
			phases[trim(phase)] = true
		end
		return phases
	end
	return nil
end

local function send_ack(to, offset, phase, detail)
	local event = { kind = "ack", phase = phase }
	if detail and detail.delivery then
		event.delivery = detail.delivery
	end
	if detail and detail.outcome then
		event.outcome = detail.outcome
	end
	local ok, encoded = pcall(maki.json.encode, {
		v = 1,
		type = "event",
		from = S.address,
		to = to,
		reply_to = offset,
		event = event,
	})
	if ok and type(encoded) == "string" then
		fire({ "publish", to, "--json", encoded })
	end
end

local function track_ack(record, offset, delivery)
	local phases = wants_ack(record)
	if not phases then
		return
	end
	if phases.received then
		send_ack(record.from, offset, "received", { delivery = delivery })
	end
	if delivery == "display" then
		-- Display is terminal: no run will follow.
		return
	end
	if not (phases.started or phases.completed) then
		return
	end
	S.pending[offset] = {
		from = record.from,
		phases = phases,
		started = false,
	}
end

local function prefix_for(record, about_id)
	local wants_reply = type(record.meta) == "table" and record.meta.reply == true
	local here = record.to or S.address
	local lines = {}
	if wants_reply then
		lines[#lines + 1] = string.format(
			"[msgq] %s asks you a question through %s",
			record.from,
			here
		)
		lines[#lines + 1] =
			'Answer them using the msgq tool: {"action":"reply","message":"<your answer>"}'
	else
		lines[#lines + 1] = string.format("[msgq] from %s on %s", record.from, here)
	end
	if about_id then
		lines[#lines + 1] = "[about " .. about_id .. "]"
	end
	return table.concat(lines, "\n")
end

local function run_command_record(cmdline, text)
	local focused = maki.session.current()
	if focused == S.session_id then
		local ok, err = maki.api.run_command(cmdline)
		if ok then
			return "turn"
		end
		S.last_error = "command refused: " .. tostring(err)
	end
	local st, err = maki.session.prompt("[msgq command, not run]\n" .. cmdline, {
		session = S.session_id,
	})
	if st then
		return st == "queued" and "queued" or "turn"
	end
	S.last_error = tostring(err)
	maki.session.notify(text, { session = S.session_id, display = "[msgq] command" })
	return "display"
end

local function deliver(record, offset)
	local body = record.message
	if type(body) ~= "string" or body == "" then
		log("warn", "ignoring a record with no message body")
		return
	end
	local from = record.from or "?"
	local rtype = record.type or "prompt"
	local about_id = nil
	if record.reply_to ~= nil then
		about_id = from .. "#" .. tostring(record.reply_to)
	end
	local text = prefix_for(record, about_id) .. "\n" .. body

	S.last_inbound = {
		from = from,
		offset = offset,
		id = S.address .. "#" .. tostring(offset),
		message = body,
	}

	local allow_act = CONFIG.policy == "honor" and trusted(from)
	local delivery

	if allow_act and rtype == "note" then
		maki.session.notify(text, {
			session = S.session_id,
			display = "[msgq] " .. from,
		})
		maki.ui.flash("[msgq] note from " .. from)
		delivery = "display"
	elseif allow_act and rtype == "command" then
		delivery = run_command_record(body, text)
	elseif allow_act then
		local st, err = maki.session.prompt(text, { session = S.session_id })
		if st then
			delivery = st == "queued" and "queued" or "turn"
		else
			S.last_error = tostring(err)
			maki.session.notify(text, {
				session = S.session_id,
				display = "[msgq] " .. from,
			})
			maki.ui.flash("[msgq] delivery failed: " .. tostring(err))
			delivery = "display"
		end
	else
		maki.session.notify(text, {
			session = S.session_id,
			display = "[msgq] " .. from,
		})
		maki.ui.flash("[msgq] " .. from .. " (display only)")
		delivery = "display"
	end

	track_ack(record, offset, delivery)
end

local function handle_record(record, offset)
	if type(record) ~= "table" then
		return
	end
	if record.v ~= 1 then
		fail("unsupported msgq protocol version: " .. tostring(record.v))
		return
	end
	local self_address = false
	for _, address in ipairs(S.addresses) do
		if record.from == address then
			self_address = true
			break
		end
	end
	if self_address then
		return -- never let a session wake itself
	end

	local followed = false
	for _, topic in ipairs(S.follows) do
		if record.to == topic then
			followed = true
			break
		end
	end
	if not followed then
		return
	end

	if record.type == "event" then
		local event = record.event
		if type(event) == "table" and event.kind == "ack" then
			local detail = ""
			if event.outcome then
				detail = " outcome=" .. tostring(event.outcome)
			elseif event.delivery then
				detail = " delivery=" .. tostring(event.delivery)
			end
			maki.ui.flash(
				string.format("[msgq] %s ack %s%s", tostring(record.from), tostring(event.phase), detail)
			)
		else
			local kind = type(event) == "table" and event.kind or "?"
			maki.ui.flash(string.format("[msgq] event %s from %s", tostring(kind), tostring(record.from)))
		end
		return
	end

	deliver(record, offset)
end

-- ---------------------------------------------------------------------------
-- Follow
-- ---------------------------------------------------------------------------

local start_follow
local schedule_restart

local function follow_on_stdout(_job, line)
	if not line or line == "" then
		return
	end
	local ok, envelope = pcall(maki.json.decode, line)
	if not ok or type(envelope) ~= "table" then
		log("warn", "could not parse a follow line")
		return
	end
	if type(envelope.record) ~= "table" or type(envelope.offset) ~= "number" then
		log("warn", "follow line has no offset; is msgq up to date?")
		return
	end
	S.restart_ms = RESTART_MS
	handle_record(envelope.record, envelope.offset)
end

local function follow_on_stderr(_job, line)
	if line and line ~= "" then
		S.follow_error = line
	end
end

local function follow_on_exit(job, code)
	if job ~= S.follow_job then
		return
	end
	S.follow_job = nil
	if S.stopping or not S.started then
		return
	end
	S.last_error = "msgq follow exited with " .. tostring(code)
	if S.follow_error then
		S.last_error = S.last_error .. ": " .. S.follow_error
		S.follow_error = nil
	end
	log("warn", S.last_error)
	schedule_restart()
end

local function stop_stale_jobs(keep)
	local jobs = maki.fn.joblist() or {}
	for _, job in ipairs(jobs) do
		local name = job.name
		if
			type(name) == "string"
			and name:sub(1, #FOLLOW_JOB_PREFIX) == FOLLOW_JOB_PREFIX
			and name ~= keep
			and job.status == "running"
		then
			pcall(maki.fn.jobstop, job.id)
		end
	end
end

start_follow = function()
	if not S.started or S.stopping then
		return
	end
	local name = FOLLOW_JOB_PREFIX .. S.consumer
	stop_stale_jobs(name)

	local job = maki.fn.jobfind(name)
	if job then
		S.follow_job = job
		maki.fn.jobattach(job, {
			on_stdout = follow_on_stdout,
			on_stderr = follow_on_stderr,
			on_exit = follow_on_exit,
		})
		log("info", "resumed following " .. S.address)
		return
	end

	local args = { "follow" }
	for _, topic in ipairs(S.follows) do
		args[#args + 1] = topic
	end
	args[#args + 1] = "--consumer"
	args[#args + 1] = S.consumer
	-- Every line is wrapped with its position and its trust verdict; the offset comes from
	-- there, and the verdict is left for a reader that wants it.
	args[#args + 1] = "--json"

	local id, err = maki.fn.jobstart(base(args), {
		name = name,
		scope = { session = S.session_id },
		on_stdout = follow_on_stdout,
		on_stderr = follow_on_stderr,
		on_exit = follow_on_exit,
		max_line_bytes = 8 * 1024 * 1024,
	})
	if not id then
		S.last_error = "could not start msgq follow: " .. tostring(err)
		log("error", S.last_error)
		maki.notify("[msgq] " .. S.last_error, "error")
		schedule_restart()
		return
	end
	S.follow_job = id
	log("info", "following " .. S.address .. " as " .. S.consumer)
end

schedule_restart = function()
	if not S.started or S.stopping then
		return
	end
	local delay = S.restart_ms or RESTART_MS
	S.restart_ms = math.min(delay * 2, MAX_RESTART_MS)
	maki.defer_fn(function()
		if S.started and not S.stopping and not S.follow_job then
			start_follow()
		end
	end, delay)
end

-- ---------------------------------------------------------------------------
-- Presence
-- ---------------------------------------------------------------------------

local function alias_text()
	if type(CONFIG.name) == "string" and CONFIG.name ~= "" then
		return (CONFIG.name:gsub("^session/", ""))
	end
	if S.address then
		return (S.address:gsub("^session/", ""))
	end
	return "maki"
end

local function presence_address()
	if S.session_id then
		return "session/" .. S.session_id
	end
	return S.address
end

local function same_card(a, b)
	local function encoded(value)
		if not value then
			return nil
		end
		local ok, text = pcall(maki.json.encode, value)
		return ok and text or nil
	end
	return encoded(a) == encoded(b)
end

-- Merge the derived locator with explicit, per-field overrides. An empty
-- override removes the field, so a derived value nobody wants (a project
-- outside git) can be cleared; an empty one is never stored as an override.
local function merge_card(derived, overrides)
	local card = {}
	for key, value in pairs(derived or {}) do
		if (overrides or {})[key] ~= "" then
			card[key] = value
		end
	end
	for key, value in pairs(overrides or {}) do
		if value ~= "" then
			card[key] = value
		end
	end
	if next(card) == nil then
		return nil
	end
	return card
end

-- Refresh the locator from a directory. Returns whether the card changed; a
-- failure leaves the session publishing presence without one.
local function refresh_card(cwd)
	if CONFIG.context == "off" then
		local changed = S.card ~= nil
		S.derived_card = nil
		S.card = nil
		return changed
	end
	if type(cwd) ~= "string" or cwd == "" then
		local ok, resolved = pcall(maki.uv.cwd)
		if not ok or type(resolved) ~= "string" or resolved == "" then
			return false
		end
		cwd = resolved
	end
	local out = run({ "card", "--cwd", cwd, "--paths", CONFIG.card_paths }, 5000)
	local decoded
	if out then
		local decoded_ok, value = pcall(maki.json.decode, out)
		if decoded_ok and type(value) == "table" then
			decoded = value
		end
	end
	local previous = S.card
	S.derived_card = decoded
	S.card = merge_card(decoded, S.card_overrides)
	return not same_card(previous, S.card)
end

local function publish_presence(state)
	if not CONFIG.presence or not S.address then
		return
	end
	local record = {
		v = 1,
		type = "event",
		from = presence_address(),
		to = PRESENCE_ADDRESS,
		event = {
			kind = "presence",
			state = state,
			ttl = CONFIG.ttl,
			participant = S.session_id,
			alias = alias_text(),
			card = S.card,
		},
	}
	local ok, encoded = pcall(maki.json.encode, record)
	if not ok then
		return
	end
	fire({ "publish", PRESENCE_ADDRESS, "--json", encoded })
end

local function set_presence_state(state)
	if S.presence_state == state then
		return
	end
	S.presence_state = state
	publish_presence(state)
end

local function start_presence()
	if not CONFIG.presence then
		return
	end
	refresh_card()
	publish_presence(S.presence_state)
	S.presence_task = maki.async.spawn(function()
		local renewals = 0
		while S.started and not S.stopping do
			maki.async.sleep(CONFIG.presence_every_ms)
			if S.started and not S.stopping then
				renewals = renewals + 1
				if renewals % 6 == 0 then
					refresh_card()
				end
				publish_presence(S.presence_state)
			end
		end
	end)
end

-- ---------------------------------------------------------------------------
-- Acknowledgements, driven by the session's own turn events
-- ---------------------------------------------------------------------------

local function on_turn_start(ev)
	if not S.started or ev.data.session_id ~= S.session_id then
		return
	end
	-- A session can start outside a repository and be pointed at one inside the
	-- first turn. The renewal timer would notice minutes later, so the locator is
	-- re-derived here, the way the pi extension does at the start of a turn.
	if refresh_card(ev.data.cwd) then
		publish_presence(S.presence_state)
	end
	for offset, pending in pairs(S.pending) do
		if pending.phases.started and not pending.started then
			pending.started = true
			send_ack(pending.from, offset, "started")
		end
	end
end

local function on_turn_end(ev)
	if not S.started or ev.data.session_id ~= S.session_id then
		return
	end
	local reason = ev.data.reason
	local outcome = "error"
	if reason == "finished" then
		outcome = "ok"
	elseif reason == "cancelled" then
		outcome = "aborted"
	end
	local done = {}
	for offset, pending in pairs(S.pending) do
		if pending.started then
			if pending.phases.completed then
				send_ack(pending.from, offset, "completed", { outcome = outcome })
			end
			done[#done + 1] = offset
		end
	end
	for _, offset in ipairs(done) do
		S.pending[offset] = nil
	end
end

local function on_status(ev)
	if not S.started or ev.data.session_id ~= S.session_id then
		return
	end
	local status = ev.data.status
	if status == "working" then
		set_presence_state("busy")
	elseif status == "needs_input" then
		set_presence_state("blocked")
	else
		set_presence_state("idle")
	end
end

local function on_cwd_changed(ev)
	if not S.started or ev.data.session_id ~= S.session_id then
		return
	end
	if refresh_card(ev.data.cwd) then
		publish_presence(S.presence_state)
	end
end

local function on_session_end(ev)
	if ev.data.session_id ~= S.session_id then
		return
	end
	local reason = ev.data.reason
	if reason == "reload" then
		return -- the follow job is session-scoped and the next instance adopts it
	end
	S.stopping = true
	S.started = false
	pcall(publish_presence, "gone")
	if S.presence_task then
		pcall(function()
			S.presence_task:cancel()
		end)
	end
	-- Leave the follow job to the host: it reaps session-owned jobs after this
	-- event, and killing it here (SIGKILL) would keep its advisory lock for its
	-- whole TTL, delaying the next instance's first delivery by up to a minute.
	S.follow_job = nil
end

local function register_autocmds()
	if #S.autocmds > 0 then
		return
	end
	S.autocmds[#S.autocmds + 1] = maki.api.create_autocmd("TurnStart", {
		callback = on_turn_start,
	})
	S.autocmds[#S.autocmds + 1] = maki.api.create_autocmd("TurnEnd", {
		callback = on_turn_end,
	})
	S.autocmds[#S.autocmds + 1] = maki.api.create_autocmd("SessionStatusChanged", {
		callback = on_status,
	})
	S.autocmds[#S.autocmds + 1] = maki.api.create_autocmd("SessionCwdChanged", {
		callback = on_cwd_changed,
	})
	S.autocmds[#S.autocmds + 1] = maki.api.create_autocmd("SessionEnd", {
		callback = on_session_end,
	})
end

-- ---------------------------------------------------------------------------
-- Identity
-- ---------------------------------------------------------------------------

local function presence_text()
	if not CONFIG.presence then
		return "off (disabled)"
	end
	return "registered (" .. S.presence_state .. ")"
end

local function job_text()
	if not S.follow_job then
		return "not running"
	end
	local info = maki.fn.jobinfo(S.follow_job)
	if not info then
		return "unknown"
	end
	return string.format("%s (pid %s)", tostring(info.status), tostring(info.pid))
end

local function trust_text()
	if CONFIG.trust == nil then
		return "(none set: every sender)"
	end
	return table.concat(CONFIG.trust, ", ")
end

local function identity_lines()
	local rows = {
		{ "session id", S.session_id or "(none)" },
		{ "sends as", S.address or "(none)" },
		{ "bus", db_path() or "~/.pi/agent/msgq/msgq.db" },
		{ "consumer", S.consumer or "(none)" },
		{ "follows", #S.follows > 0 and table.concat(S.follows, ", ") or "(none)" },
		{ "policy", CONFIG.policy },
		{ "trust", trust_text() },
		{ "presence", presence_text() },
		{ "follow job", job_text() },
		{ "last error", S.last_error or "(none)" },
	}
	if S.card then
		for _, key in ipairs({ "project", "branch", "worktree", "path" }) do
			local value = S.card[key]
			if type(value) == "string" and value ~= "" then
				rows[#rows + 1] = { key, value }
			end
		end
	end
	local lines = { S.address or "(none: the bus did not start)" }
	for _, row in ipairs(rows) do
		lines[#lines + 1] = string.format("  %-11s %s", row[1], row[2])
	end
	return lines
end

-- ---------------------------------------------------------------------------
-- Human commands
-- ---------------------------------------------------------------------------

local function send_from_command(to, message)
	local ack = to ~= S.address and "completed" or nil
	local out, err = publish_now(to, message, { type = "prompt", ack = ack })
	if not out then
		fail("could not send: " .. tostring(err))
		return
	end
	maki.ui.flash("msgq: sent to " .. to)
	local entries = list_peers()
	if entries then
		local listening = false
		for _, entry in ipairs(entries) do
			if entry.address == to then
				listening = true
				break
			end
		end
		if not listening then
			maki.notify("nothing is listening at " .. to .. " right now", "warn")
		end
	end
end

local function split_peer_args(raw)
	local value = trim(raw or "")
	if value == "" then
		return nil, nil
	end
	local quote = value:sub(1, 1)
	if quote == '"' or quote == "'" then
		if value:sub(-1) == quote then
			value = value:sub(2, -2)
		end
		return nil, trim(value)
	end
	local address, rest = value:match("^(%S+)%s*(.*)$")
	if rest == "" then
		return address, nil
	end
	local q = rest:sub(1, 1)
	if (q == '"' or q == "'") and rest:sub(-1) == q then
		rest = rest:sub(2, -2)
	end
	return address, trim(rest)
end

local function require_started()
	if S.started then
		return true
	end
	maki.notify("msgq is not running: " .. (S.last_error or "no focused session"), "warn")
	return false
end

maki.api.register_command({
	name = "/peers",
	description = "Who is reachable on the bus, and send one of them a message. '/peers me' reports this session's identity.",
	nargs = "?",
	handler = function(opts)
		if not require_started() then
			return
		end
		local arg = trim(opts.args or "")
		if arg == "me" then
			show_text("msgq identity", identity_lines())
			return
		end
		if arg ~= "" then
			maki.notify("usage: /peers [me]", "warn")
			return
		end
		local entries, err = list_peers()
		if not entries then
			maki.notify("could not read the bus: " .. tostring(err), "warn")
			return
		end
		local address = pick_peer(others(entries))
		if not address then
			return
		end
		local message = compose()
		if not message then
			return
		end
		send_from_command(address, message)
	end,
})

maki.api.register_command({
	name = "/peer",
	description = "Send a message to a peer: /peer <address> [message]. 'me' is this session, a bare name is session/<name>.",
	nargs = "*",
	handler = function(opts)
		if not require_started() then
			return
		end
		local raw_address, message = split_peer_args(opts.args)
		local to
		if raw_address then
			local resolved, err = resolve_address(raw_address)
			if not resolved then
				maki.notify(err, "warn")
				return
			end
			to = resolved
		else
			local entries, err = list_peers()
			if not entries then
				maki.notify("could not read the bus: " .. tostring(err), "warn")
				return
			end
			to = pick_peer(others(entries))
			if not to then
				return
			end
		end
		local body = message or compose()
		if not body then
			return
		end
		send_from_command(to, body)
	end,
})

maki.api.register_command({
	name = "/reply",
	description = "Answer the message you most recently received: /reply [message]",
	nargs = "*",
	handler = function(opts)
		if not require_started() then
			return
		end
		local inbound = S.last_inbound
		if not inbound then
			maki.notify("nothing to reply to: no message has been received yet", "warn")
			return
		end
		local body = trim(opts.args or "")
		if body == "" then
			body = compose()
		end
		if not body then
			return
		end
		local out, err = publish_now(inbound.from, body, {
			type = "prompt",
			reply_to = inbound.id,
		})
		if not out then
			fail("could not reply: " .. tostring(err))
			return
		end
		maki.ui.flash("msgq: replied to " .. inbound.from)
	end,
})

maki.api.register_command({
	name = "/msgq",
	description = "Alias for '/peers me': this session's bus address and health.",
	nargs = 0,
	handler = function()
		show_text("msgq identity", identity_lines())
	end,
})

-- ---------------------------------------------------------------------------
-- Agent tool
-- ---------------------------------------------------------------------------

maki.api.register_prompt_hint({
	slot = "tool_usage",
	content = "- A message starting with `[msgq]` comes from another process, not from the user."
		.. " When it asks a question, answer it with the `msgq` tool:"
		.. ' `{"action":"reply","message":"<your answer>"}`.',
})

maki.api.register_tool({
	name = "msgq",
	permission = "run",
	kind = "network",
	description = [[Send and receive messages on the local msgq bus.

Messages starting with `[msgq]` in this session arrived through the bus. Use this tool to answer
one (`reply`) or to message another process or session (`send`). `list` shows who is live on the
bus right now. `card` reports or changes this session's locator (project, branch, worktree, path);
omit `card` to read it. Addresses look like `session/<name>`, `cli/<who>@<host>`, or `group/<name>`;
a bare name is a group.]],
	schema = {
		type = "object",
		properties = {
			action = {
				type = "string",
				enum = { "send", "reply", "list", "card" },
				description = "send: publish to a peer. reply: answer the message received most recently. list: who is live. card: read or change this session's locator.",
				required = true,
			},
			to = {
				type = "string",
				description = "Recipient address for send. Bare names become group/<name>; use session/<name> for a session.",
			},
			message = { type = "string", description = "Message body for send or reply." },
			type = {
				type = "string",
				enum = { "prompt", "steer", "follow_up", "note" },
				description = "What the recipient should do with it. Default: prompt.",
			},
			card = {
				type = "object",
				description = "Locator overrides for the card action. An empty string removes that field, derived value included.",
				properties = {
					project = { type = "string" },
					branch = { type = "string" },
					worktree = { type = "string" },
					path = { type = "string" },
				},
			},
		},
		required = { "action" },
	},
	handler = function(input)
		if not S.started then
			return {
				llm_output = "error: msgq is not running: " .. (S.last_error or "no focused session"),
				is_error = true,
			}
		end
		local action = input.action

		if action == "card" then
			if input.card == nil then
				if S.card then
					return { llm_output = maki.json.encode(S.card) }
				end
				return { llm_output = "no locator card is set" }
			end
			local allowed = { project = true, branch = true, worktree = true, path = true }
			for key, value in pairs(input.card) do
				if not allowed[key] then
					return {
						llm_output = "error: unknown card field '" .. tostring(key)
							.. "'; expected project, branch, worktree, path",
						is_error = true,
					}
				end
				if type(value) ~= "string" then
					return {
						llm_output = "error: card field '" .. tostring(key) .. "' must be a string",
						is_error = true,
					}
				end
				local trimmed = trim(value)
				if trimmed == "" then
					-- An empty string is a removal, not a no-op: it must also clear a
					-- derived field, so it is kept as an empty override.
					S.card_overrides[key] = ""
				else
					S.card_overrides[key] = trimmed
				end
			end
			-- Overrides are merged even with context derivation off, so a session that
			-- opted out of derivation can still describe itself.
			refresh_card()
			S.card = merge_card(S.derived_card, S.card_overrides)
			publish_presence(S.presence_state)
			if S.card then
				return { llm_output = "card updated\n" .. maki.json.encode(S.card) }
			end
			return { llm_output = "card cleared" }
		end

		if action == "list" then
			local entries, err = list_peers()
			if not entries then
				return { llm_output = "error: could not read the bus: " .. tostring(err), is_error = true }
			end
			local lines = { S.address .. " (this session)" }
			for _, entry in ipairs(entries) do
				local label = "  " .. entry.address .. "  " .. roster_tail(entry)
				if entry.address == S.address then
					label = label .. "  (this session)"
				end
				lines[#lines + 1] = label
			end
			if #lines == 1 then
				lines[#lines + 1] = "  no other participant is reachable on the bus"
			end
			return { llm_output = table.concat(lines, "\n") }
		end

		if type(input.message) ~= "string" or trim(input.message) == "" then
			return { llm_output = "error: message is required", is_error = true }
		end

		if action == "reply" then
			local inbound = S.last_inbound
			if not inbound then
				return {
					llm_output = "error: nothing to reply to: no message has been received yet",
					is_error = true,
				}
			end
			local out, err = publish_now(inbound.from, input.message, {
				type = "prompt",
				reply_to = inbound.id,
			})
			if not out then
				return { llm_output = "error: could not reply: " .. tostring(err), is_error = true }
			end
			return { llm_output = "replied to " .. inbound.from }
		end

		if action == "send" then
			local to, err = resolve_bus_address(input.to)
			if not to then
				return { llm_output = "error: " .. tostring(err), is_error = true }
			end
			local ack = to ~= S.address and "completed" or nil
			local out, failure = publish_now(to, input.message, {
				type = input.type or "prompt",
				ack = ack,
			})
			if not out then
				return { llm_output = "error: could not send: " .. tostring(failure), is_error = true }
			end
			return { llm_output = "sent to " .. to }
		end

		return { llm_output = "error: unknown action: " .. tostring(action), is_error = true }
	end,
})

-- ---------------------------------------------------------------------------
-- Lifecycle
-- ---------------------------------------------------------------------------

function M.setup(opts)
	opts = opts or {}
	local allowed = {
		name = true,
		policy = true,
		trust = true,
		presence = true,
		bin = true,
		db = true,
		topics = true,
		ttl = true,
		presence_every_ms = true,
		context = true,
		card_paths = true,
	}
	for key, value in pairs(opts) do
		if allowed[key] then
			CONFIG[key] = value
		end
	end
	CONFIG.policy = CONFIG.policy == "display" and "display" or "honor"
	CONFIG.context = CONFIG.context == "off" and "off" or "auto"
	if
		CONFIG.card_paths ~= "relative"
		and CONFIG.card_paths ~= "full"
		and CONFIG.card_paths ~= "none"
	then
		CONFIG.card_paths = "relative"
	end

	if type(CONFIG.trust) == "string" then
		local list = {}
		for value in CONFIG.trust:gmatch("[^,]+") do
			list[#list + 1] = trim(value)
		end
		CONFIG.trust = #list > 0 and list or nil
	elseif type(CONFIG.trust) == "table" then
		local list = {}
		for _, value in ipairs(CONFIG.trust) do
			list[#list + 1] = trim(value)
		end
		CONFIG.trust = #list > 0 and list or nil
	else
		CONFIG.trust = nil
	end

	if type(CONFIG.topics) == "string" then
		local list = {}
		for value in CONFIG.topics:gmatch("[^,]+") do
			list[#list + 1] = normalize_topic(trim(value))
		end
		CONFIG.topics = list
	elseif type(CONFIG.topics) == "table" then
		local list = {}
		for _, value in ipairs(CONFIG.topics) do
			list[#list + 1] = normalize_topic(trim(value))
		end
		CONFIG.topics = list
	end

	-- Nothing here may touch maki.fn or the session: init.lua runs while the
	-- plugin host is loading, and a roundtrip there can wait forever. The start
	-- is deferred below, after the load has unwound.
end

function M.start()
	if S.started then
		return
	end
	local session, err = maki.session.current()
	if not session then
		S.last_error = "no focused session: " .. tostring(err)
		log("warn", S.last_error)
		return
	end
	log("info", "starting for session " .. tostring(session))

	S.session_id = session
	-- The session id is both the sender identity and the unique address. A
	-- configured name is only an extra address to listen on: sending as the
	-- alias would make every instance look like every other one (and make its
	-- own messages look self-sent), which silently drops traffic between them.
	local id_address = "session/" .. session
	S.addresses = { id_address }
	S.address = id_address
	local name = CONFIG.name
	if type(name) == "string" and name ~= "" then
		local alias = name:find("/", 1, true) and name or ("session/" .. name)
		if alias ~= id_address then
			S.addresses[#S.addresses + 1] = alias
		end
	end
	-- Unique per session: two instances may follow the same alias, and native
	-- fan-out gives each its own cursor instead of a lock conflict.
	S.consumer = "maki#" .. session:gsub("[/%s]", "-")

	S.follows = {}
	for _, address in ipairs(S.addresses) do
		S.follows[#S.follows + 1] = address
	end
	for _, topic in ipairs(CONFIG.topics or {}) do
		S.follows[#S.follows + 1] = topic
	end

	S.started = true
	S.stopping = false
	S.restart_ms = RESTART_MS
	S.last_error = nil
	S.pending = {}
	S.derived_card = nil
	S.card = nil
	S.card_overrides = {}

	register_autocmds()
	start_follow()
	start_presence()
end

function M.stop()
	if not S.started then
		return
	end
	S.stopping = true
	S.started = false
	pcall(publish_presence, "gone")
	if S.presence_task then
		pcall(function()
			S.presence_task:cancel()
		end)
		S.presence_task = nil
	end
	if S.follow_job then
		pcall(maki.fn.jobstop, S.follow_job)
		S.follow_job = nil
	end
	for _, id in ipairs(S.autocmds) do
		pcall(maki.api.del_autocmd, id)
	end
	S.autocmds = {}
end

function M.identity()
	return identity_lines()
end

-- Start only after the whole config dir has loaded, on a background task so a
-- slow roundtrip cannot freeze the UI. A start during init.lua would call into
-- the session while the host is still loading, which is what hung /reload the
-- first time. Retry a few times when there is no focused session yet.
local function start_task()
	for _ = 1, 5 do
		local ok, err = pcall(M.start)
		if not ok then
			log("error", "start failed: " .. tostring(err))
			maki.notify("[msgq] start failed: " .. tostring(err), "error")
			return
		end
		if S.started then
			return
		end
		maki.async.sleep(500)
	end
end

maki.defer_fn(function()
	local ok, err = pcall(maki.async.spawn, start_task)
	if not ok then
		log("error", "could not schedule the start: " .. tostring(err))
	end
end, 150)

return M
