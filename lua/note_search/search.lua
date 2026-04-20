local M = {}

--- Configuration for the note_search picker
M.config = {
	-- Path to the note_search binary
	note_search_cmd = "note_search",
	-- Database path (optional, uses env var or default if not set)
	database_path = nil,
	-- Note directory path (optional, uses env var or default if not set)
	note_dir = nil,
}

--- State to store last interactive search for repeating
M.last_search = {
	attribute = nil,
	value = nil,
	text = nil,
}

--- Setup function to configure the picker
function M.setup(opts)
	M.config = vim.tbl_deep_extend("force", M.config, opts or {})
end

--- Execute note_search command and return results
--- Build the base command list for note_search
local function build_cmd()
	local cmd = { M.config.note_search_cmd }
	if M.config.database_path then
		table.insert(cmd, "-d")
		table.insert(cmd, M.config.database_path)
	end
	return cmd
end

--- Execute note_search command and return results
local function execute_note_search(args)
	local cmd = build_cmd()
	for _, arg in ipairs(args) do
		table.insert(cmd, arg)
	end

	local result = vim.system(cmd):wait()
	if result.code ~= 0 then
		vim.notify("note_search failed: " .. (result.stderr or ""), vim.log.levels.ERROR)
		return nil
	end

	local results = {}
	for line in (result.stdout or ""):gmatch("[^\r\n]+") do
		table.insert(results, line)
	end
	return results
end

--- Search notes with filters and open in Snacks picker
function M.search_notes(opts)
	opts = opts or {}

	local args = { "notes" }

	if opts.tags then
		table.insert(args, "--tags")
		table.insert(args, opts.tags)
	end

	if opts.links then
		table.insert(args, "--links")
		table.insert(args, opts.links)
	end

	if opts.attributes then
		table.insert(args, "--attributes")
		table.insert(args, opts.attributes)
	end

	if opts.text then
		table.insert(args, "--text")
		table.insert(args, opts.text)
	end

	if opts.search_body then
		table.insert(args, "--search-body")
		table.insert(args, opts.search_body)
	end

	if opts.date_range then
		table.insert(args, "--date-range")
		table.insert(args, opts.date_range)
	end

	if opts.start_date then
		table.insert(args, "--start-date")
		table.insert(args, opts.start_date)
	end

	if opts.end_date then
		table.insert(args, "--end-date")
		table.insert(args, opts.end_date)
	end

	table.insert(args, "--absolute-path")
	table.insert(args, "--sort")
	table.insert(args, "created")

	local results = execute_note_search(args)
	if not results or #results == 0 then
		vim.notify("No notes found", vim.log.levels.INFO)
		return
	end

	Snacks.picker.pick({
		title = "Note Search",
		items = vim.tbl_map(function(line)
			local filename, line_num, text

			filename, line_num, text = line:match('^"([^"]+)":(%d+)%s+(.*)$')

			if not filename then
				filename = line:match("^(.+)%s+%[%d+%s+todos")
				if not filename then
					filename = line:match("^(.+)%s+%[")
				end
				if filename then
					line_num = 1
					text = filename
				else
					filename = line
					line_num = 1
					text = line
				end
			end

			return {
				text = text or filename,
				file = filename,
				pos = { tonumber(line_num) or 1, 0 },
			}
		end, results),
		format = "file",
		preview = "file",
		confirm = function(picker, item)
			picker:close()
			if item then
				vim.cmd("edit " .. vim.fn.fnameescape(item.file))
				if item.pos then
					vim.api.nvim_win_set_cursor(0, { item.pos[1], item.pos[2] })
				end
			end
		end,
	})
end

--- Get list of all tags
function M.get_tags()
	local results = execute_note_search({ "values", "tag" })
	if not results then
		return {}
	end

	-- Skip header line "Unique values for 'tag':"
	local tags = {}
	for i, line in ipairs(results) do
		if i > 1 and line:match("^%s+") then
			table.insert(tags, line:match("^%s+(.+)$"))
		end
	end
	return tags
end

--- Get list of all links
function M.get_links()
	local results = execute_note_search({ "values", "link" })
	if not results then
		return {}
	end

	local links = {}
	for i, line in ipairs(results) do
		if i > 1 and line:match("^%s+") then
			table.insert(links, line:match("^%s+(.+)$"))
		end
	end
	return links
end

--- Get list of all attribute names
function M.get_attribute_names()
	local results = execute_note_search({ "attributes" })
	if not results then
		return {}
	end

	local attributes = {}
	for i, line in ipairs(results) do
		if i > 1 and line:match("^%s+") then
			table.insert(attributes, line:match("^%s+(.+)$"))
		end
	end
	return attributes
end

--- Get values for a specific attribute
function M.get_attribute_values(attr_name)
	local results = execute_note_search({ "values", "attr:" .. attr_name })
	if not results then
		return {}
	end

	local values = {}
	for i, line in ipairs(results) do
		if i > 1 and line:match("^%s+") then
			table.insert(values, line:match("^%s+(.+)$"))
		end
	end
	return values
end

--- Interactive picker: Select attribute first, then value, then search
function M.interactive_search()
	-- Step 1: Select attribute
	local attributes = M.get_attribute_names()
	if #attributes == 0 then
		vim.notify("No attributes found in database", vim.log.levels.WARN)
		return
	end

	Snacks.picker.select(attributes, {
		prompt = "Select Attribute",
	}, function(attr)
		if not attr then
			return
		end

		-- Step 2: Select value for the attribute
		local values = M.get_attribute_values(attr)
		if #values == 0 then
			vim.notify("No values found for attribute: " .. attr, vim.log.levels.WARN)
			return
		end

		Snacks.picker.select(values, {
			prompt = "Select Value for " .. attr,
		}, function(val)
			if not val then
				return
			end

			-- Step 3: Input text to search within results
			vim.ui.input({
				prompt = "Search text (optional, press Enter to skip): ",
				default = "",
			}, function(input)
				-- Step 4: Save search parameters for repeat functionality
				M.last_search.attribute = attr
				M.last_search.value = val
				M.last_search.text = input

				-- Step 5: Perform the search
				local search_opts = {
					attributes = attr .. "=" .. val,
				}
				if input and input ~= "" then
					search_opts.text = input
				end
				M.search_notes(search_opts)
			end)
		end)
	end)
end

--- Interactive picker: Select attribute first, then value, then todo
function M.interactive_todo()
	-- Step 1: Select attribute
	local attributes = M.get_attribute_names()
	if #attributes == 0 then
		vim.notify("No attributes found in database", vim.log.levels.WARN)
		return
	end

	Snacks.picker.select(attributes, {
		prompt = "Select Attribute",
	}, function(attr)
		if not attr then
			return
		end

		-- Step 2: Select value for the attribute
		local values = M.get_attribute_values(attr)
		if #values == 0 then
			vim.notify("No values found for attribute: " .. attr, vim.log.levels.WARN)
			return
		end

		Snacks.picker.select(values, {
			prompt = "Select Value for " .. attr,
		}, function(val)
			if not val then
				return
			end

			-- Step 3: Input text to search within results
			vim.ui.input({
				prompt = "Search text (optional, press Enter to skip): ",
				default = "",
			}, function(input)
				-- Step 4: Save search parameters for repeat functionality
				M.last_search.attribute = attr
				M.last_search.value = val
				M.last_search.text = input

				-- Step 5: Perform the search
				local search_opts = {
					attributes = attr .. "=" .. val,
				}
				if input and input ~= "" then
					search_opts.text = input
				end
				search_opts.open = true
				M.search_todos(search_opts)
			end)
		end)
	end)
end

--- Repeat the last interactive search with same parameters
function M.repeat_interactive_search()
	if not M.last_search.attribute or not M.last_search.value then
		vim.notify("No previous interactive search to repeat", vim.log.levels.WARN)
		return
	end

	-- Reconstruct the search options
	local search_opts = {
		attributes = M.last_search.attribute .. "=" .. M.last_search.value,
	}

	if M.last_search.text and M.last_search.text ~= "" then
		search_opts.text = M.last_search.text
	end

	-- Show what we're searching
	local msg = "Repeating search: " .. M.last_search.attribute .. "=" .. M.last_search.value
	if M.last_search.text and M.last_search.text ~= "" then
		msg = msg .. " (text: " .. M.last_search.text .. ")"
	end
	vim.notify(msg, vim.log.levels.INFO)

	-- Perform the search
	M.search_notes(search_opts)
end

--- Quick search by tag
function M.search_by_tag()
	local tags = M.get_tags()
	if #tags == 0 then
		vim.notify("No tags found in database", vim.log.levels.WARN)
		return
	end

	Snacks.picker.select(tags, {
		prompt = "Select Tag",
	}, function(tag)
		if tag then
			M.search_notes({ tags = tag })
		end
	end)
end

--- Build the base note_search command and args
local function build_note_search_cmd()
	local cmd = M.config.note_search_cmd
	local args = {}
	if M.config.database_path then
		table.insert(args, "-d")
		table.insert(args, M.config.database_path)
	end
	return cmd, args
end

--- Parse a note_search output line and extract the filename
local function parse_note_line(line)
	-- Try todo format: "/path/file.md":LINENUM text
	local filename, line_num = line:match('^"([^"]+)":(%d+)')
	if filename then
		return filename, tonumber(line_num)
	end

	-- Try note format: /path/file.md [N todos, M links]
	filename = line:match("^(.+)%s+%[%d+%s+todos")
	if not filename then
		filename = line:match("^(.+)%s+%[")
	end
	if filename then
		return filename, 1
	end

	-- Fallback
	return line, 1
end

--- Search with live input using Snacks proc finder
function M.live_search()
	local base_cmd, base_args = build_note_search_cmd()

	Snacks.picker({
		title = "Note Search",
		live = true,
		supports_live = true,
		finder = function(opts, ctx)
			-- Build args: notes --absolute-path --list [--text <search>]
			local args = vim.list_extend({}, base_args)
			vim.list_extend(args, { "notes", "--absolute-path", "--list" })

			local search = ctx.filter.search
			if search and search ~= "" then
				vim.list_extend(args, { "--text", search })
			end

			return require("snacks.picker.source.proc").proc({
				cmd = base_cmd,
				args = args,
				transform = function(item)
					local filename, line_num = parse_note_line(item.text)
					item.file = filename
					item.text = filename
					item.pos = { line_num, 0 }
				end,
			}, ctx)
		end,
		format = "file",
		preview = "file",
		confirm = function(picker, item)
			picker:close()
			if item and item.file and item.file ~= "" then
				vim.cmd("edit " .. vim.fn.fnameescape(item.file))
				if item.pos then
					vim.api.nvim_win_set_cursor(0, { item.pos[1], item.pos[2] })
				end
			end
		end,
	})
end

--- Search todos with filters
function M.search_todos(opts)
	opts = opts or {}

	local args = { "todos" }

	if opts.priority then
		table.insert(args, "--priority")
		table.insert(args, opts.priority)
	end

	if opts.due_date then
		table.insert(args, "--due-date")
		table.insert(args, opts.due_date)
	end

	if opts.open then
		table.insert(args, "--open")
	elseif opts.closed then
		table.insert(args, "--closed")
	end

	if opts.tags then
		table.insert(args, "--tags")
		table.insert(args, opts.tags)
	end

	if opts.text then
		table.insert(args, "--text")
		table.insert(args, opts.text)
	end

	if opts.attributes then
		table.insert(args, "--attributes")
		table.insert(args, opts.attributes)
	end

	table.insert(args, "--absolute-path")

	local results = execute_note_search(args)
	if not results or #results == 0 then
		vim.notify("No todos found", vim.log.levels.INFO)
		return
	end

	Snacks.picker.pick({
		title = "Todo Search",
		items = vim.tbl_map(function(line)
			-- Parse line format for todos: ""/path/file.md":LINENUM text"
			-- Parse line format for notes: "/path/file.md [X todos, Y links]"
			local filename, line_num, text

			-- Try todo format first (quoted filename with line number)
			filename, line_num, text = line:match('^"([^"]+)":(%d+)%s+(.*)$')

			if not filename then
				-- Try note format (filename followed by bracket info)
				-- Pattern: match everything up to " [N todos, M links]"
				filename = line:match("^(.+)%s+%[%d+%s+todos")
				if not filename then
					-- Alternative pattern for bracket format
					filename = line:match("^(.+)%s+%[")
				end
				if filename then
					line_num = 1
					text = filename
				else
					-- Fallback: use whole line as filename
					filename = line
					line_num = 1
					text = line
				end
			end

			return {
				text = text or filename,
				file = filename,
				pos = { tonumber(line_num) or 1, 0 },
			}
		end, results),
		format = "file",
		preview = "file",
		confirm = function(picker, item)
			picker:close()
			if item then
				vim.cmd("edit " .. vim.fn.fnameescape(item.file))
				if item.pos then
					vim.api.nvim_win_set_cursor(0, { item.pos[1], item.pos[2] })
				end
			end
		end,
	})
end

--- Extract the wiki link under the cursor (e.g., [[NeoVimNote]])
local function get_link_under_cursor()
	local row, col = unpack(vim.api.nvim_win_get_cursor(0))
	local line = vim.api.nvim_buf_get_lines(0, row - 1, row, false)[1] or ""

	local start = 1
	while true do
		local s, e = line:find("%[%[.-%]%]", start)
		if not s then
			break
		end
		local name = line:sub(s + 2, e - 2)
		-- Lua indices are 1-based, nvim col is 0-based
		if col >= s - 1 and col <= e - 1 then
			return name
		end
		start = e + 1
	end
	return nil
end

--- Get the wiki link under the cursor and search for backlinks
function M.search_backlinks()
	local link_name = get_link_under_cursor()

	if not link_name then
		local row, col = unpack(vim.api.nvim_win_get_cursor(0))
		local line = vim.api.nvim_buf_get_lines(0, row - 1, row, false)[1] or ""
		local column_content = line:sub(1, col + 1)
		local before = column_content:match("(%S*)$")
		if before and #before > 0 then
			link_name = before:gsub("%.md$", "")
		end
	end

	if not link_name or #link_name == 0 then
		vim.notify("No link found under cursor", vim.log.levels.WARN)
		return
	end

	link_name = link_name:gsub("%.md$", "")
	M.search_notes({ links = link_name })
end

--- Command completion helpers
function M.complete_tags()
	return M.get_tags()
end

function M.complete_links()
	return M.get_links()
end

function M.complete_attributes()
	return M.get_attribute_names()
end

--- Format a timestamp as YYYYMMDD for note_search CLI
local function format_date(ts)
	return os.date("%Y%m%d", ts)
end

--- Search notes created today
function M.search_today(opts)
	opts = opts or {}
	opts.date_range = "today"
	M.search_notes(opts)
end

--- Search notes created this week
function M.search_this_week(opts)
	opts = opts or {}
	opts.date_range = "this_week"
	M.search_notes(opts)
end

--- Search notes created in the last 4 weeks
function M.search_last_4_weeks(opts)
	opts = opts or {}
	local now = os.time()
	local four_weeks_ago = now - 28 * 24 * 3600
	opts.start_date = format_date(four_weeks_ago)
	opts.end_date = format_date(now)
	opts.date_range = nil
	M.search_notes(opts)
end

--- Interactive picker for recent notes (today / this week / last 4 weeks)
function M.search_recent()
	local ranges = {
		{ label = "Today", fn = M.search_today },
		{ label = "This Week", fn = M.search_this_week },
		{ label = "Last 4 Weeks", fn = M.search_last_4_weeks },
	}

	Snacks.picker.select(
		vim.tbl_map(function(r)
			return r.label
		end, ranges),
		{
			prompt = "Recent Notes",
		},
		function(selected)
			if not selected then
				return
			end
			for _, r in ipairs(ranges) do
				if r.label == selected then
					r.fn()
					return
				end
			end
		end
	)
end

--- Get all note names from database (lowercase, space-separated)
function M.get_note_names()
	local results = execute_note_search({ "list-names" })
	if not results then
		return {}
	end

	local names = {}
	for i, line in ipairs(results) do
		if i > 1 and line:match("^%s+") then
			names[line:match("^%s+(.+)$")] = true
		else
			names[line] = true
		end
	end
	return names
end

local function countSet(t)
	local count = 0
	for _ in pairs(t) do
		count = count + 1
	end
	return count
end

--- Replace text phrases with links to notes in current buffer
function M.replace_links()
	local note_names = M.get_note_names()
	if countSet(note_names) == 0 then
		vim.notify("No notes found in database", vim.log.levels.INFO)
		return
	end

	local bufnr = vim.api.nvim_get_current_buf()
	local lines = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)

	-- Convert note_names set to a list and sort by length (longest first)
	local sorted_names = {}
	for name, _ in pairs(note_names) do
		table.insert(sorted_names, name)
	end
	table.sort(sorted_names, function(a, b)
		return #a > #b
	end)

	-- Process each line
	local new_lines = {}
	for _, line in ipairs(lines) do
		local result = line
		local replaced = {}

		for _, note_name in ipairs(sorted_names) do
			-- Build a case-insensitive pattern that matches spaces as spaces, underscores, or hyphens
			local pattern = ""
			for i = 1, #note_name do
				local char = note_name:sub(i, i)
				if char == "_" then
					pattern = pattern .. "[_%- ]"
				elseif char == "-" then
					pattern = pattern .. "[_%- ]"
				elseif char == " " then
					pattern = pattern .. "[_%- ]"
				else
					local lower = char:lower()
					local upper = char:upper()
					if lower ~= upper then
						pattern = pattern .. "[" .. lower .. upper .. "]"
					else
						pattern = pattern .. char
					end
				end
			end

			-- Find all non-overlapping matches and replace them
			local pos = 1
			local new_result = ""
			while pos <= #result do
				local start_pos, end_pos = result:find(pattern, pos)
				if not start_pos then
					new_result = new_result .. result:sub(pos)
					break
				end

				-- Check if this range overlaps with any already replaced range
				local overlaps = false
				for _, r in ipairs(replaced) do
					if start_pos <= r.end_pos and end_pos >= r.start_pos then
						overlaps = true
						break
					end
				end

				-- Check word boundaries: character before must be non-word or start of line
				-- and character after must be non-word or end of line
				local has_word_boundary_before = start_pos == 1
					or not result:sub(start_pos - 1, start_pos - 1):match("[%w_%-]")
				local has_word_boundary_after = end_pos == #result
					or not result:sub(end_pos + 1, end_pos + 1):match("[%w_%-]")

				if overlaps or not has_word_boundary_before or not has_word_boundary_after then
					new_result = new_result .. result:sub(pos, start_pos)
					pos = start_pos + 1
				else
					new_result = new_result .. result:sub(pos, start_pos - 1)
					new_result = new_result .. "[[" .. note_name .. "]]"
					table.insert(replaced, { start_pos = start_pos, end_pos = end_pos })
					pos = end_pos + 1
				end
			end
			result = new_result
		end

		table.insert(new_lines, result)
	end

	vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, new_lines)
end

--- Jump to the file and line from an agenda todo line
--- The cursor can be anywhere on the line containing the markdown link
function M.jump_to_agenda_todo()
	local line = vim.api.nvim_get_current_line()

	-- Find markdown link pattern: ([LinkName](</path/to/file.md:line>))
	-- Pattern explanation:
	-- %[%s*       - match literal [ with optional whitespace
	-- ([^%]]+)    - capture group 1: everything until ]
	-- %]%s*       - match literal ] with optional whitespace
	-- %(%s*       - match literal ( with optional whitespace
	-- <            - match literal <
	-- ([^>]+)      - capture group 2: everything until > (the file path)
	-- :(%d+)       - capture group 3: colon followed by digits (line number)
	-- >            - match literal >
	-- %)           - match literal )
	local pattern = "%[([^%]]+)%]%s*%(%s*<([^>]+):(%d+)>%)"

	local link_text, file_path, line_num = line:match(pattern)

	if not file_path or not line_num then
		vim.notify("No file link found on this line", vim.log.levels.WARN)
		return
	end

	-- Trim whitespace from file_path
	file_path = file_path:match("^%s*(.-)%s*$")

	local lnum = tonumber(line_num)
	if not lnum then
		vim.notify("Invalid line number", vim.log.levels.ERROR)
		return
	end

	-- Check if file exists
	local stat = vim.loop.fs_stat(file_path)
	if not stat then
		vim.notify("File not found: " .. file_path, vim.log.levels.ERROR)
		return
	end

	-- Open the file
	vim.cmd("edit " .. vim.fn.fnameescape(file_path))

	-- Jump to the line
	vim.api.nvim_win_set_cursor(0, { lnum, 0 })

	-- Center the line on screen
	vim.cmd("normal! zz")

	vim.notify(string.format("Jumped to %s:%d", file_path, lnum), vim.log.levels.INFO)
end

--- Create and open agenda in a temporary buffer
function M.open_agenda_buffer()
	-- Get the database path from environment or use default
	local db_path = os.getenv("NOTE_SEARCH_DATABASE") or "./note.sqlite"

	-- Run the agenda command
	local cmd = string.format("note_search -d '%s' agenda 2>&1", db_path:gsub("'", "'\\''"))
	local handle = io.popen(cmd)
	if not handle then
		vim.notify("Failed to run agenda command", vim.log.levels.ERROR)
		return
	end

	local output = handle:read("*a")
	handle:close()

	if not output or output == "" then
		vim.notify("No agenda content generated", vim.log.levels.WARN)
		return
	end

	-- Create a new buffer
	local buf = vim.api.nvim_create_buf(false, true)

	-- Set buffer options
	vim.api.nvim_set_option_value("buftype", "nofile", { buf = buf })
	vim.api.nvim_set_option_value("bufhidden", "wipe", { buf = buf })
	vim.api.nvim_set_option_value("swapfile", false, { buf = buf })
	vim.api.nvim_set_option_value("filetype", "markdown", { buf = buf })

	-- Split lines and set content
	local lines = vim.split(output, "\n", { plain = true })
	vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)

	-- Open the buffer in the current window
	vim.api.nvim_set_current_buf(buf)

	-- Set buffer name
	local date = os.date("%Y-%m-%d")
	vim.api.nvim_buf_set_name(buf, string.format("Agenda-%s.md", date))

	-- Move cursor to top
	vim.api.nvim_win_set_cursor(0, { 1, 0 })

	vim.notify(string.format("Agenda generated for %s", date), vim.log.levels.INFO)
end

--- Mark a todo as done from the agenda buffer
--- Opens the file in background, marks todo as done, appends checkmark and date
function M.mark_agenda_todo_done()
	local line = vim.api.nvim_get_current_line()

	-- Find markdown link pattern: ([LinkName](</path/to/file.md:line>))
	local pattern = "%[([^%]]+)%]%s*%(%s*<([^>]+):(%d+)>%)"
	local link_text, file_path, line_num = line:match(pattern)

	if not file_path or not line_num then
		vim.notify("No file link found on this line", vim.log.levels.WARN)
		return
	end

	-- Trim whitespace from file_path
	file_path = file_path:match("^%s*(.-)%s*$")

	local lnum = tonumber(line_num)
	if not lnum then
		vim.notify("Invalid line number", vim.log.levels.ERROR)
		return
	end

	-- Check if file exists
	local stat = vim.loop.fs_stat(file_path)
	if not stat then
		vim.notify("File not found: " .. file_path, vim.log.levels.ERROR)
		return
	end

	-- Read the file
	local lines = {}
	local f = io.open(file_path, "r")
	if not f then
		vim.notify("Failed to open file: " .. file_path, vim.log.levels.ERROR)
		return
	end
	for l in f:lines() do
		table.insert(lines, l)
	end
	f:close()

	-- Check if line exists
	if lnum > #lines then
		vim.notify("Line number out of range", vim.log.levels.ERROR)
		return
	end

	-- Get the current line content
	local todo_line = lines[lnum]

	-- Check if it's a todo item
	if not todo_line:match("^%- %[ %]") then
		vim.notify("This line is not an open todo item", vim.log.levels.WARN)
		return
	end

	-- Mark as done: change "- [ ]" to "- [x]"
	-- Match "- [" followed by optional whitespace, then capture that whitespace,
	-- then match "]", and replace with "- [", the whitespace, "x]"
	-- local new_line = todo_line:gsub("^(%- %[)(%s*)%]", "%1%2x]")
	local new_line = todo_line:gsub("^(%- %[)(%s*)%]", "%1x]")

	-- Append checkmark and date link if not already present
	local date_str = os.date("%Y-%m-%d")
	if not new_line:match("%s+✅") then
		new_line = new_line .. " ✅ [[" .. date_str .. "]]"
	end

	-- Update the line
	lines[lnum] = new_line

	-- Write the file back
	local f_out = io.open(file_path, "w")
	if not f_out then
		vim.notify("Failed to write file: " .. file_path, vim.log.levels.ERROR)
		return
	end
	for _, l in ipairs(lines) do
		f_out:write(l .. "\n")
	end
	f_out:close()

	-- Update the current agenda line to reflect the change
	local current_buf = vim.api.nvim_get_current_buf()
	local cursor_pos = vim.api.nvim_win_get_cursor(0)
	local row = cursor_pos[1]

	-- Mark the agenda line as done visually
	local agenda_line = vim.api.nvim_buf_get_lines(current_buf, row - 1, row, false)[1]
	if agenda_line then
		agenda_line = agenda_line:gsub("^(%- %[)(%s*)%]", "%1x]")
		if not agenda_line:match("%s+✅") then
			agenda_line = agenda_line .. " ✅ [[" .. date_str .. "]]"
		end
		vim.api.nvim_buf_set_lines(current_buf, row - 1, row, false, { agenda_line })
	end

	vim.notify(string.format("Marked as done: %s:%d", file_path, lnum), vim.log.levels.INFO)
end

--- Generate agenda for the current note
--- Gets the current buffer's filename and generates agenda for that note
function M.open_agenda_for_current_note()
	-- Get the current buffer's filename
	local current_file = vim.api.nvim_buf_get_name(0)
	if not current_file or current_file == "" then
		vim.notify("No file associated with current buffer", vim.log.levels.WARN)
		return
	end

	-- Extract just the filename without path and extension
	local note_name = vim.fn.fnamemodify(current_file, ":t:r")
	if not note_name or note_name == "" then
		vim.notify("Could not extract note name from filename", vim.log.levels.WARN)
		return
	end

	-- Read the current buffer content to extract frontmatter
	local lines = vim.api.nvim_buf_get_lines(0, 0, -1, false)
	local content = table.concat(lines, "\n")

	-- Extract frontmatter type
	local note_type = nil
	if content:sub(1, 3) == "---" then
		-- Find the end of frontmatter
		local frontmatter_end = content:find("\n---", 4)
		if frontmatter_end then
			local frontmatter = content:sub(4, frontmatter_end)
			-- Look for type: field in frontmatter
			note_type = frontmatter:match("type:%s*([^\n]+)")
			if note_type then
				note_type = note_type:gsub("%s", "") -- Remove whitespace
			end
		end
	end

	-- Determine which flag to use based on type
	local type_flag = ""
	if note_type == "company" then
		type_flag = "-C"
	elseif note_type == "person" then
		type_flag = "-E"
	elseif note_type == "department" then
		type_flag = "-D"
	end

	-- Get the database path from environment or use default
	local db_path = os.getenv("NOTE_SEARCH_DATABASE") or "./note.sqlite"

	-- Build the agenda command with the appropriate flag
	local cmd
	if type_flag ~= "" then
		cmd = string.format(
			"note_search -d '%s' agenda %s '%s' 2>&1",
			db_path:gsub("'", "'\\''"),
			type_flag,
			note_name:gsub("'", "'\\''")
		)
	else
		cmd = string.format(
			"note_search -d '%s' agenda '%s' 2>&1",
			db_path:gsub("'", "'\\''"),
			note_name:gsub("'", "'\\''")
		)
	end

	local handle = io.popen(cmd)
	if not handle then
		vim.notify("Failed to run agenda command", vim.log.levels.ERROR)
		return
	end

	local output = handle:read("*a")
	handle:close()

	if not output or output == "" then
		vim.notify(string.format("No agenda found for note: %s", note_name), vim.log.levels.WARN)
		return
	end

	-- Create a new buffer
	local buf = vim.api.nvim_create_buf(false, true)

	-- Set buffer options
	vim.api.nvim_set_option_value("buftype", "nofile", { buf = buf })
	vim.api.nvim_set_option_value("bufhidden", "wipe", { buf = buf })
	vim.api.nvim_set_option_value("swapfile", false, { buf = buf })
	vim.api.nvim_set_option_value("filetype", "markdown", { buf = buf })

	-- Split lines and set content
	local output_lines = vim.split(output, "\n", { plain = true })
	vim.api.nvim_buf_set_lines(buf, 0, -1, false, output_lines)

	-- Open the buffer in the current window
	vim.api.nvim_set_current_buf(buf)

	-- Set buffer name
	local date = os.date("%Y-%m-%d")
	vim.api.nvim_buf_set_name(buf, string.format("Agenda-%s-%s.md", note_name, date))

	-- Move cursor to top
	vim.api.nvim_win_set_cursor(0, { 1, 0 })

	vim.notify(
		string.format("Agenda generated for %s (type: %s)", note_name, note_type or "project"),
		vim.log.levels.INFO
	)
end

--- Search for files in the note directory using Snacks files picker
function M.search_files_in_notes()
	local notes_dir = M.config.note_dir or os.getenv("NOTE_SEARCH_DIR") or "."
	notes_dir = vim.fn.resolve(vim.fn.expand(notes_dir))

	local stat = vim.loop.fs_stat(notes_dir)
	if not stat then
		vim.notify("Note directory not found: " .. notes_dir, vim.log.levels.ERROR)
		return
	end

	Snacks.picker.files({
		cwd = notes_dir,
		title = "Search Files in Notes",
		confirm = function(picker, item)
			picker:close()
			if item and item.file then
				vim.cmd("edit " .. vim.fn.fnameescape(notes_dir .. "/" .. item.file))
			end
		end,
	})
end

--- Open a Snacks picker listing all notes that reference the current note
function M.references()
	local current_name = vim.fn.expand("%:t:r")

	if current_name == "" then
		vim.notify("No file open", vim.log.levels.WARN)
		return
	end

	local results = execute_note_search({ "backlinks", current_name })

	if not results or #results == 0 then
		vim.notify("No references to " .. current_name, vim.log.levels.INFO)
		return
	end

	local notes_dir = M.config.note_dir or os.getenv("NOTE_SEARCH_DIR") or "."
	notes_dir = vim.fn.resolve(vim.fn.expand(notes_dir))

	Snacks.picker.pick({
		title = "References to " .. current_name,
		items = vim.tbl_map(function(line)
			local filename = line:match("^%s*(.+)$") or line
			local full_path = notes_dir .. "/" .. filename
			if filename:match("^/") then
				full_path = filename
			end
			return {
				text = filename,
				file = full_path,
				pos = { 1, 0 },
			}
		end, results),
		format = "file",
		preview = "file",
		confirm = function(picker, item)
			picker:close()
			if item then
				vim.cmd("edit " .. vim.fn.fnameescape(item.file))
				if item.pos then
					vim.api.nvim_win_set_cursor(0, { item.pos[1], item.pos[2] })
				end
			end
		end,
	})
end

return M
