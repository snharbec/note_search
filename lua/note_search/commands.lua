local M = {}

-- Function to get the word under cursor and check if it's a JIRA issue key
local function get_jira_issue_under_cursor()
	local line = vim.api.nvim_get_current_line()
	local col = vim.api.nvim_win_get_cursor(0)[2]
	
	-- Find word boundaries
	local word_start = col
	local word_end = col
	
	-- Move back to find start of word
	while word_start > 0 do
		local char = line:sub(word_start, word_start)
		if not char:match("[A-Za-z0-9%-]") then
			word_start = word_start + 1
			break
		end
		word_start = word_start - 1
	end
	if word_start == 0 then word_start = 1 end
	
	-- Move forward to find end of word
	while word_end <= #line do
		local char = line:sub(word_end + 1, word_end + 1)
		if char == "" or not char:match("[A-Za-z0-9%-]") then
			break
		end
		word_end = word_end + 1
	end
	
	local word = line:sub(word_start, word_end)
	
	-- Check if word matches JIRA issue pattern: [A-Z]+-[0-9]+
	if word:match("^[A-Z]+-[0-9]+$") then
		return word, word_start, word_end
	end
	
	return nil, nil, nil
end

-- Function to download JIRA issue and convert to link
local function jira_issue_to_link()
	local issue_key, start_col, end_col = get_jira_issue_under_cursor()
	
	if not issue_key then
		vim.notify("No issue found", vim.log.levels.WARN)
		return
	end
	
	local cfg = require("note_search").config
	local notes_dir = cfg.notes_dir
	
	-- Run note_search jira-issue command
	vim.notify("Fetching JIRA issue: " .. issue_key, vim.log.levels.INFO)
	
	local cmd = string.format("note_search jira-issue \"%s\" -o \"%s\"", issue_key, notes_dir)
	local output = vim.fn.system(cmd)
	
	if vim.v.shell_error ~= 0 then
		vim.notify("Failed to fetch JIRA issue: " .. output, vim.log.levels.ERROR)
		return
	end
	
	-- Replace the text under cursor with [[ISSUE_KEY]]
	local line = vim.api.nvim_get_current_line()
	local new_line = line:sub(1, start_col - 1) .. "[[" .. issue_key .. "]]" .. line:sub(end_col + 1)
	vim.api.nvim_set_current_line(new_line)
	
	vim.notify("Converted " .. issue_key .. " to link and saved to jira/", vim.log.levels.INFO)
end

function M.setup(cfg)
	local types_mod = require("note_search.types")
	local linker = require("note_search.linker")
	local expander = require("note_search.expander")
	local search = require("note_search.search")
	local exec = require("note_search.markdown-exec")
	exec.setup({})

	vim.api.nvim_create_user_command("NoteType", function(opts)
		local args = vim.split(opts.args, " ", { plain = false })
		local type_name = args[1]
		types_mod.create_type(type_name)
	end, { nargs = "+" })

	vim.api.nvim_create_user_command("NoteTypeNote", function(opts)
		local args = vim.split(opts.args, " ", { plain = false })
		local type_name = args[1]
		local subtype_name = args[2]
		types_mod.note(type_name, subtype_name)
	end, { nargs = "*" })

	vim.api.nvim_create_user_command("NoteTypeInsertLink", function(opts)
		local args = vim.split(opts.args, " ", { plain = false })
		local type_name = args[1]
		expander.insert_link_to_note_type(type_name)
	end, { nargs = "+" })

	vim.api.nvim_create_user_command("NoteTypeInsertLinkAll", function()
		expander.insert_link_to_note()
	end, {})

	vim.api.nvim_create_user_command("NoteTypeInsertFile", function()
		expander.insert_link_to_file()
	end, {})

	vim.api.nvim_create_user_command("NoteTypeInsertBlock", function()
		expander.insert_selection()
	end, {})

	vim.api.nvim_create_user_command("NoteTypeInsertLinkRecent4Weeks", function()
		expander.insert_link_to_recent_4_weeks()
	end, {})

	vim.api.nvim_create_user_command("NoteTypeInsertLinkCurrentWeek", function()
		expander.insert_link_to_current_week()
	end, {})

	vim.api.nvim_create_user_command("NoteTypeInsertLinkToday", function()
		expander.insert_link_to_today()
	end, {})

	-- Create or open a day note for a specific date using template.lua
	local template_mod = require("note_search.template")

	local function create_day_note()
		local cfg = require("note_search").config
		local notes_dir = cfg.notes_dir

		vim.ui.input({
			prompt = "Enter date (YYYY-MM-DD): ",
			default = os.date("%Y-%m-%d"),
		}, function(input)
			if not input or input == "" then
				return
			end

			local year, month, day = input:match("^(%d%d%d%d)%-(%d%d)%-(%d%d)$")
			if not year then
				vim.notify("Invalid date format. Use YYYY-MM-DD", vim.log.levels.ERROR)
				return
			end

			local month_names = { "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec" }
			local month_name = month_names[tonumber(month)]
			local file_path = string.format("%s/daily/%s/%s/%s.md", notes_dir, year, month_name, input)

			local stat = vim.loop.fs_stat(file_path)
			if stat then
				vim.cmd("edit " .. vim.fn.fnameescape(file_path))
			else
				local template_path = notes_dir .. "/templates/daily.md"
				local custom_time = os.time({ year = tonumber(year), month = tonumber(month), day = tonumber(day) })

				local content = template_mod.render(template_path, { title = input }, custom_time)

				if not content then
					-- Template doesn't exist, create a default one with all variables
					content = "# {{title}}\n\n---\ncreated: {{today}}\ntype: daily\n---\n\n"
					content = content:gsub("{{title}}", input)
					content = content:gsub("{{today}}", input)
				end

				local dir_path = string.format("%s/daily/%s/%s", notes_dir, year, month_name)
				vim.fn.mkdir(dir_path, "p")

				local fd = io.open(file_path, "w")
				if fd then
					fd:write(content)
					fd:close()
					vim.cmd("edit " .. vim.fn.fnameescape(file_path))
					vim.notify("Created day note: " .. input, vim.log.levels.INFO)
				else
					vim.notify("Failed to create day note", vim.log.levels.ERROR)
				end
			end
		end)
	end

	vim.api.nvim_create_user_command("NoteCreateDayNote", function()
		create_day_note()
	end, {})

	vim.api.nvim_create_user_command("NoteBacklinks", function()
		local name = vim.fn.expand("%:t:r")
		local links = linker.get_backlinks(name)
		if #links == 0 then
			return vim.notify("No backlinks found")
		end
		vim.fn.setqflist({}, "r", {
			title = "Backlinks: " .. name,
			items = vim.tbl_map(function(f)
				return { filename = f, text = f }
			end, links),
		})
		vim.cmd("copen")
	end, {})

	vim.api.nvim_create_user_command("NoteSearchBacklinks", function()
		search.search_backlinks()
	end, {})

	vim.api.nvim_create_user_command("NoteSearchAgendaJump", function()
		search.jump_to_agenda_todo()
	end, {})

	vim.api.nvim_create_user_command("NoteSearchAgenda", function()
		search.open_agenda_buffer()
	end, {})

	vim.api.nvim_create_user_command("NoteSearchAgendaDone", function()
		search.mark_agenda_todo_done()
	end, {})

	vim.api.nvim_create_user_command("NoteSearchAgendaCurrent", function()
		search.open_agenda_for_current_note()
	end, {})

	vim.api.nvim_create_user_command("NoteSearchFiles", function()
		search.search_files_in_notes()
	end, {})

	vim.api.nvim_create_user_command("NoteReferences", function()
		search.references()
	end, {})

	local function is_mapped(mode, lhs)
		return vim.fn.maparg(lhs, mode) ~= ""
	end
	if is_mapped("n", cfg.keymap_group) then
		vim.keymap.del("n", cfg.keymap_group)
	end
	local function nmap(key, command, desc)
		vim.keymap.set({ "n", "v" }, cfg.keymap_group .. key, command, { desc = desc })
	end
	nmap("t", function()
		types_mod.note("daily")
	end, "Daily note")
	nmap("n", function()
		Snacks.picker.files({ cwd = cfg.notes_dir })
	end, "Open note")
	nmap("Nc", function()
		Snacks.picker.files({ cwd = cfg.notes_dir .. "/" .. cfg.types.company.dir })
	end, "Open company note")
	nmap("Np", function()
		Snacks.picker.files({ cwd = cfg.notes_dir .. "/" .. cfg.types.project.dir })
	end, "Open project note")
	nmap("Ne", function()
		Snacks.picker.files({ cwd = cfg.notes_dir .. "/" .. cfg.types.person.dir })
	end, "Open person note")
	nmap("Nd", function()
		Snacks.picker.files({ cwd = cfg.notes_dir .. "/" .. cfg.types.department.dir })
	end, "Open department note")
	nmap("Nt", function()
		Snacks.picker.files({ cwd = cfg.notes_dir .. "/" .. cfg.types.daily.dir })
	end, "Open daily note")
	nmap("P", function()
		types_mod.create_type("project")
	end, "Project Search / Create ")
	nmap("E", function()
		types_mod.create_type("person")
	end, "Person Search / Create")
	nmap("C", function()
		types_mod.create_type("company")
	end, "Company Search / Create")
	nmap("R", function()
		types_mod.create_type("department")
	end, "Department Search / Create")
	nmap("e", function()
		types_mod.note("person")
	end, "Person note")
	nmap("p", function()
		types_mod.note("project")
	end, "Project note")
	nmap("c", function()
		types_mod.note("company")
	end, "Company note")
	nmap("r", function()
		types_mod.note("department")
	end, "Department note")
	nmap("ss", function()
		search.interactive_search()
	end, "Search for attributes in notes")
	nmap("s.", function()
		search.repeat_interactive_search()
	end, "Repeat last search")
	nmap("st", function()
		search.search_by_tag()
	end, "Note Search by Tag")
	nmap("sl", function()
		search.live_search()
	end, "Note Search (Live)")
	nmap("sb", function()
		search.search_notes({ search_body = vim.fn.input("Search in body: ") })
	end, "Search in body")
	nmap("sB", function()
		search.search_backlinks()
	end, "Search backlinks under cursor")
	nmap("sR", function()
		search.references()
	end, "Notes referencing current note")
	nmap("sx", function()
		search.interactive_todo()
	end, "Search in todos")
	nmap("sr", function()
		search.search_recent()
	end, "Search recent notes")
	nmap("sd", function()
		search.search_today()
	end, "Search today's notes")
	nmap("sw", function()
		search.search_this_week()
	end, "Search this week's notes")
	nmap("s4", function()
		search.search_last_4_weeks()
	end, "Search last 4 weeks notes")
	nmap("l", function()
		require("note_search.search").replace_links()
	end, "Replace text with links to notes")
	nmap("g", function()
		require("note_search.search").jump_to_agenda_todo()
	end, "Jump to agenda todo file and line")
	nmap("A", function()
		require("note_search.search").open_agenda_buffer()
	end, "Open agenda in temporary buffer")
	nmap("D", function()
		require("note_search.search").mark_agenda_todo_done()
	end, "Mark agenda todo as done")
	nmap("a", function()
		require("note_search.search").open_agenda_for_current_note()
	end, "Open agenda for current note")
	nmap("sf", function()
		require("note_search.search").search_files_in_notes()
	end, "Search files in notes directory")
	nmap("T", "<cmd>NoteCreateDayNote<cr>", "Create or open day note")
	nmap("j", function()
		jira_issue_to_link()
	end, "Download JIRA issue and convert to link")
	expander.register_inserter_normal()
end

return M
