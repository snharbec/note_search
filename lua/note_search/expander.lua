local M = {}

local function pick_note_link(opts)
	local cfg = require("note_search").config
	Snacks.picker.files({
		cwd = opts.cwd,
		title = opts.title,
		cmd = cfg.find_command,
		args = opts.args,
		matcher = {
			fuzzy = true,
			frecency = true,
			history_bonus = true,
			ignore_case = true,
		},
		file = {
			filename_first = true,
			git_status = false,
			truncate = "center",
		},
		actions = {
			confirm = function(picker, item)
				picker:close()
				local basename = vim.fn.fnamemodify(item.file, ":t"):gsub("%.md$", "")
				local row, col = unpack(vim.api.nvim_win_get_cursor(0))
				local text = "[[" .. basename .. "]]"
				vim.api.nvim_buf_set_text(0, row - 1, col, row - 1, col, { text .. "  " })
				vim.api.nvim_win_set_cursor(0, { row, col + #text + 5 })
				vim.schedule(function()
					vim.cmd("startinsert")
				end)
			end,
		},
	})
end

function M.insert_link_to_note_type(sub_element)
	local cfg = require("note_search").config
	pick_note_link({
		cwd = cfg.notes_dir .. "/" .. sub_element,
		title = "Search " .. sub_element,
		args = { "--max-depth", "1" },
	})
end

function M.insert_link_to_note()
	local cfg = require("note_search").config
	pick_note_link({
		cwd = cfg.notes_dir,
		title = "Search note",
		args = nil,
	})
end

function M.insert_link_to_recent_4_weeks()
	local cfg = require("note_search").config
	-- Calculate timestamp for 4 weeks ago (28 days)
	local four_weeks_ago = os.time() - (28 * 24 * 60 * 60)
	local date_str = os.date("%Y-%m-%d", four_weeks_ago)

	Snacks.picker.files({
		cwd = cfg.notes_dir,
		title = "Search recent notes (last 4 weeks)",
		cmd = cfg.find_command,
		args = { "--newer", date_str },
		matcher = {
			fuzzy = true,
			frecency = true,
			history_bonus = true,
			ignore_case = true,
		},
		file = {
			filename_first = true,
			git_status = false,
			truncate = "center",
		},
		actions = {
			confirm = function(picker, item)
				picker:close()
				local basename = vim.fn.fnamemodify(item.file, ":t"):gsub("%.md$", "")
				local row, col = unpack(vim.api.nvim_win_get_cursor(0))
				local text = "[[" .. basename .. "]]"
				vim.api.nvim_buf_set_text(0, row - 1, col, row - 1, col, { text .. "  " })
				vim.api.nvim_win_set_cursor(0, { row, col + #text + 5 })
				vim.schedule(function()
					vim.cmd("startinsert")
				end)
			end,
		},
	})
end

function M.insert_link_to_current_week()
	local cfg = require("note_search").config
	-- Calculate timestamp for 7 days ago (current week)
	local week_ago = os.time() - (7 * 24 * 60 * 60)
	local date_str = os.date("%Y-%m-%d", week_ago)

	Snacks.picker.files({
		cwd = cfg.notes_dir,
		title = "Search notes (current week)",
		cmd = cfg.find_command,
		args = { "--newer", date_str },
		matcher = {
			fuzzy = true,
			frecency = true,
			history_bonus = true,
			ignore_case = true,
		},
		file = {
			filename_first = true,
			git_status = false,
			truncate = "center",
		},
		actions = {
			confirm = function(picker, item)
				picker:close()
				local basename = vim.fn.fnamemodify(item.file, ":t"):gsub("%.md$", "")
				local row, col = unpack(vim.api.nvim_win_get_cursor(0))
				local text = "[[" .. basename .. "]]"
				vim.api.nvim_buf_set_text(0, row - 1, col, row - 1, col, { text .. "  " })
				vim.api.nvim_win_set_cursor(0, { row, col + #text + 5 })
				vim.schedule(function()
					vim.cmd("startinsert")
				end)
			end,
		},
	})
end

function M.insert_link_to_today()
	local cfg = require("note_search").config
	-- Get today's date at midnight
	local today_str = os.date("%Y-%m-%d")
	
	Snacks.picker.files({
		cwd = cfg.notes_dir,
		title = "Search notes (today)",
		cmd = cfg.find_command,
		args = { "--newer", today_str },
		matcher = {
			fuzzy = true,
			frecency = true,
			history_bonus = true,
			ignore_case = true,
		},
		file = {
			filename_first = true,
			git_status = false,
			truncate = "center",
		},
		actions = {
			confirm = function(picker, item)
				picker:close()
				local basename = vim.fn.fnamemodify(item.file, ":t"):gsub("%.md$", "")
				local row, col = unpack(vim.api.nvim_win_get_cursor(0))
				local text = "[[" .. basename .. "]]"
				vim.api.nvim_buf_set_text(0, row - 1, col, row - 1, col, { text .. "  " })
				vim.api.nvim_win_set_cursor(0, { row, col + #text + 5 })
				vim.schedule(function()
					vim.cmd("startinsert")
				end)
			end,
		},
	})
end

function M.insert_link_to_file()
	local cfg = require("note_search").config
	local abs_path = cfg.abs_path
	if abs_path and #abs_path > 0 then
		local name = vim.fn.fnamemodify(abs_path, ":t")
		local row, col = unpack(vim.api.nvim_win_get_cursor(0))
		local text = "[" .. name .. "](" .. abs_path .. ")"
		vim.api.nvim_buf_set_text(0, row - 1, col, row - 1, col, { text .. "  " })
		vim.api.nvim_win_set_cursor(0, { row, col + #text + 5 })
		vim.cmd("startinsert")
	end
end

M.insert_selection = function()
	local cfg = require("note_search").config
	if cfg.selected and #cfg.selected > 0 then
		local row, col = unpack(vim.api.nvim_win_get_cursor(0))
		row = row - 1
		local text = { "```" .. cfg.file_type }
		text = vim.list_extend(text, cfg.selected)
		table.insert(text, "```")
		table.insert(text, "")
		vim.api.nvim_buf_set_text(0, row, col, row, col, text)
		vim.api.nvim_win_set_cursor(0, { row, col + #text + 5 })
	else
		print("No selection")
	end
	vim.cmd("startinsert")
end

local imap = function(lhs, rhs, desc)
	local cfg = require("note_search").config
	vim.keymap.set("i", cfg.insert_group .. lhs, rhs, { desc = desc, buffer = true })
end

local nmap = function(lhs, rhs, desc)
	local cfg = require("note_search").config
	local leader = vim.g.maplocalleader or "\\"
	if leader == "leadery" then
		return
	end
	vim.keymap.set("n", "<localleader>" .. lhs, rhs, { desc = desc })
end

function M.register_smart_inserter()
	imap("p", "<C-o>:NoteTypeInsertLink project<CR>")
	imap("e", "<C-o>:NoteTypeInsertLink person<CR>")
	imap("c", "<C-o>:NoteTypeInsertLink company<CR>")
	imap("d", "<C-o>:NoteTypeInsertLinkToday<CR>")
	imap("D", "<C-o>:NoteTypeInsertLink department<CR>")
	imap("t", "<C-o>:NoteTypeInsertLink daily<CR>")
	imap("f", "<C-o>:NoteTypeInsertFile<CR>")
	imap("n", "<C-o>:NoteTypeInsertLinkAll<CR>")
	imap("w", "<C-o>:NoteTypeInsertLinkCurrentWeek<CR>")
	imap("4", "<C-o>:NoteTypeInsertLinkRecent4Weeks<CR>")
	imap("b", "<C-o>:NoteTypeInsertBlock<CR>")
end

function M.register_inserter_normal()
	nmap("np", "<cmd>NoteTypeInsertLink project<cr>", "Insert link to project")
	nmap("ne", "<cmd>NoteTypeInsertLink person<cr>", "Insert link to person")
	nmap("nc", "<cmd>NoteTypeInsertLink company<cr>", "Insert link to company")
	nmap("nD", "<cmd>NoteTypeInsertLink department<cr>", "Insert link to department")
	nmap("nt", "<cmd>NoteTypeInsertLink daily<cr>", "Insert link to daily")
	nmap("nf", "<cmd>NoteTypeInsertFile<cr>", "Insert link to file")
	nmap("nn", "<cmd>NoteTypeInsertLinkAll<cr>", "Insert link to note")
	nmap("nd", "<cmd>NoteTypeInsertLinkToday<cr>", "Insert link to recent note (today)")
	nmap("nw", "<cmd>NoteTypeInsertLinkCurrentWeek<cr>", "Insert link to recent note (current week)")
	nmap("n4", "<cmd>NoteTypeInsertLinkRecent4Weeks<cr>", "Insert link to recent note (4 weeks)")
	nmap("nb", "<cmd>NoteTypeInsertBlock<cr>", "Insert block")
end

return M
