local M = {}

function M.setup(cfg)
	local types_mod = require("note_search.types")
	local linker = require("note_search.linker")
	local expander = require("note_search.expander")
	local search = require("note_search.search")

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
	expander.register_inserter_normal()
end

return M
