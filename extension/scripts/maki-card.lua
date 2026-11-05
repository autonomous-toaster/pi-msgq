-- Locator card behaviour of the msgq maki plugin, driven from the outside.
--
-- The plugin is loaded against a stubbed `maki` table, so this harness exercises the real
-- source: the card it derives, the overrides the agent tool applies, and the presence record
-- it publishes. The `msgq` CLI is stubbed where the plugin shells out.
--
-- Usage (from `extension/scripts`): MSGQ_SOURCE=.../maki/lua/msgq.lua lua maki-card.lua

local root = os.getenv("MSGQ_SOURCE")
assert(root, "MSGQ_SOURCE must point at maki/lua/msgq.lua")

-- A minimal JSON codec: the plugin only encodes and decodes small flat tables.
local function encode(value)
	local kind = type(value)
	if kind == "string" then
		return '"' .. value:gsub('"', '\\"'):gsub("\n", "\\n") .. '"'
	elseif kind == "number" or kind == "boolean" then
		return tostring(value)
	elseif kind == "table" then
		local keys = {}
		for key in pairs(value) do
			keys[#keys + 1] = key
		end
		table.sort(keys)
		local parts = {}
		for _, key in ipairs(keys) do
			parts[#parts + 1] = encode(tostring(key)) .. ":" .. encode(value[key])
		end
		return "{" .. table.concat(parts, ",") .. "}"
	end
	return "null"
end

local function decode(text)
	local pos = 1
	local function skip()
		pos = text:find("[^%s]", pos) or (#text + 1)
	end
	local parse_value
	local function parse_string()
		pos = pos + 1
		local out = {}
		while pos <= #text do
			local char = text:sub(pos, pos)
			if char == '"' then
				pos = pos + 1
				return table.concat(out)
			end
			if char == "\\" then
				pos = pos + 1
				char = text:sub(pos, pos)
			end
			out[#out + 1] = char
			pos = pos + 1
		end
		error("unterminated string")
	end
	function parse_value()
		skip()
		local char = text:sub(pos, pos)
		if char == "{" then
			pos = pos + 1
			local out = {}
			skip()
			if text:sub(pos, pos) == "}" then
				pos = pos + 1
				return out
			end
			while true do
				skip()
				local key = parse_string()
				skip()
				pos = pos + 1 -- ':'
				out[key] = parse_value()
				skip()
				char = text:sub(pos, pos)
				pos = pos + 1
				if char == "}" then
					return out
				end
			end
		elseif char == '"' then
			return parse_string()
		elseif char == "t" then
			pos = pos + 4
			return true
		elseif char == "f" then
			pos = pos + 5
			return false
		elseif char == "n" then
			pos = pos + 4
			return nil
		end
		local number = text:match("^-?%d+%.?%d*[eE]?[-+]?%d*", pos)
		pos = pos + #number
		return tonumber(number)
	end
	return parse_value()
end

-- What the plugin sees of the outside world.
local H = {
	derived = {
		project = "github.com/org/repo",
		branch = "main",
		worktree = "repo",
		path = ".",
	},
	jobs = {}, -- every CLI invocation, in order
	published = {}, -- every presence record that reached the bus
	tools = {},
	handlers = {}, -- autocmd name -> callback
	waited = 0,
}

local function run_job(argv, on_stdout)
	local args = {}
	for index = 2, #argv do
		args[#args + 1] = argv[index]
	end
	if args[1] == "publish" then
		for _, arg in ipairs(args) do
			if arg:sub(1, 1) == "{" then
				H.published[#H.published + 1] = decode(arg)
			end
		end
		return "group/presence#1\n"
	elseif args[1] == "card" then
		return encode(H.derived) .. "\n"
	elseif args[1] == "sessions" then
		return ""
	end
	return ""
end

maki = {
	json = { encode = encode, decode = decode },
	async = {
		spawn = function(fn)
			coroutine.resume(coroutine.create(fn))
			return { cancel = function() end }
		end,
		-- The renewal loop is an infinite task; yielding from it keeps the harness finite.
		sleep = function()
			coroutine.yield()
		end,
	},
	uv = {
		cwd = function()
			return "/repo"
		end,
		os_getenv = function()
			return nil
		end,
	},
	fn = {
		jobstart = function(argv, opts)
			opts = opts or {}
			local id = #H.jobs + 1
			H.jobs[id] = { argv = argv, name = opts.name }
			-- A job that waits for output is answered at once; a publish does not wait,
			-- but its record still reaches the bus.
			if opts.on_stdout then
				opts.on_stdout(id, run_job(argv))
			else
				run_job(argv)
			end
			return id
		end,
		-- Waiting reads the job's stream, which is how a synchronous `run` gets stdout.
		jobwait = function(id)
			H.waited = H.waited + 1
			return { exit_code = 0, stdout = run_job(H.jobs[id].argv), stderr = "" }
		end,
		jobstop = function() end,
		jobforget = function() end,
		jobinfo = function()
			return nil
		end,
		joblist = function()
			return {}
		end,
		jobfind = function()
			return nil
		end,
		jobattach = function() end,
	},
	log = { info = function() end, warn = function() end, error = function() end },
	notify = function() end,
	defer_fn = function(fn)
		fn()
	end,
	api = {
		create_autocmd = function(name, opts)
			H.handlers[name] = opts.callback
			return { name = name }
		end,
		del_autocmd = function() end,
		register_tool = function(tool)
			H.tools[tool.name] = tool
			return function() end
		end,
		register_command = function() end,
		register_prompt_hint = function() end,
	},
	session = {
		current = function()
			return "01a1card"
		end,
		notify = function() end,
		prompt = function() end,
	},
	ui = {
		input = function()
			return nil
		end,
		flash = function() end,
	},
}

local plugin = assert(loadfile(root))()
plugin.setup({ presence = true, card_paths = "relative" })

local function last_record()
	return H.published[#H.published]
end

local function last_card()
	for index = #H.published, 1, -1 do
		local event = H.published[index].event
		if event and event.card ~= nil then
			return event.card
		end
	end
	return nil
end

local function same(a, b)
	local function normalise(value)
		if value == nil then
			return {}
		end
		return value
	end
	return encode(normalise(a)) == encode(normalise(b))
end

local function call_tool(input)
	return H.tools.msgq.handler(input)
end

local checks = {}
local function check(name, ok, detail)
	checks[#checks + 1] = { name = name, ok = ok, detail = detail }
end

-- Running a session: start publishes presence, and the first turn re-derives the card.
plugin.start()
check("start publishes presence with a card", last_record() and last_card() ~= nil, encode(last_record()))
check(
	"the card is the one the CLI derived",
	same(last_card(), H.derived),
	encode(last_card())
)

-- The host moved the session into a repository between start and its first turn.
H.derived = { project = "github.com/org/repo", branch = "feature/card", worktree = "repo", path = "." }
H.handlers.TurnStart({ data = { session_id = "01a1card", cwd = "/repo" } })
check(
	"the first turn re-derives the card",
	last_card() and last_card().branch == "feature/card",
	encode(last_card())
)

-- The agent reads and then narrows its locator.
local read = call_tool({ action = "card" })
check("omitting the card reads it", read.llm_output:find("feature/card", 1, true) ~= nil, read.llm_output)
call_tool({ action = "card", card = { branch = "review" } })
check("an override replaces one field", last_card() and last_card().branch == "review", encode(last_card()))

-- Outside git the derived project is the directory name; an empty string removes it again.
H.derived = { project = "scratch", path = "." }
call_tool({ action = "card", card = { project = "" } })
check(
	"an empty override removes the derived field",
	last_card() and last_card().project == nil,
	encode(last_card())
)

-- An override is published even when derivation is off, so a session can still describe itself.
plugin.stop()
plugin.setup({ presence = true, card_paths = "relative", context = "off" })
plugin.start()
call_tool({ action = "card", card = { project = "org/hand-run" } })
check(
	"an override survives context off",
	last_card() and last_card().project == "org/hand-run",
	encode(last_card())
)

-- An unknown field is refused rather than published.
local refused = call_tool({ action = "card", card = { status = "busy" } })
check("an unknown card field is refused", refused.is_error == true, encode(refused))

local failures = 0
for _, entry in ipairs(checks) do
	if not entry.ok then
		failures = failures + 1
	end
	io.write(string.format("%s %s%s\n", entry.ok and "ok  " or "FAIL", entry.name, entry.ok and "" or ("  " .. tostring(entry.detail))))
end
io.write(string.format("%d checks, %d failed\n", #checks, failures))
os.exit(failures == 0 and 0 or 1)
