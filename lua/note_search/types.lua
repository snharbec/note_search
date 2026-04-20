local M = {}
local template = require("note_search.template")

local en_month_short = {
	"Jan",
	"Feb",
	"Mar",
	"Apr",
	"May",
	"Jun",
	"Jul",
	"Aug",
	"Sep",
	"Oct",
	"Nov",
	"Dec",
}
local en_month_full = {
	"January",
	"February",
	"March",
	"April",
	"May",
	"June",
	"July",
	"August",
	"September",
	"October",
	"November",
	"December",
}

local function os_date_en(fmt, time)
	time = time or os.time()
	local month_num = tonumber(os.date("%m", time))
	fmt = fmt:gsub("%%B", en_month_full[month_num])
	fmt = fmt:gsub("%%b", en_month_short[month_num])
	return os.date(fmt, time)
end

local function position_cursor(pattern)
	local lines = vim.api.nvim_buf_get_lines(0, 0, -1, false)
	for i, line in ipairs(lines) do
		local start_col, end_col = string.find(line, pattern)
		if start_col then
			lines[i] = string.gsub(line, pattern, " ")
			vim.api.nvim_buf_set_lines(0, 0, -1, false, lines)
			vim.api.nvim_win_set_cursor(0, { i, start_col - 1 })
			return
		end
	end
end

local function get_selection()
	local vstart, vend = vim.fn.getpos("v"), vim.fn.getpos(".")
	local min, max = math.min(vstart[2], vend[2]), math.max(vstart[2], vend[2])
	return vim.fn.getline(min, max)
end

local function save_original_position()
	local cfg = require("note_search").config
	local abs_path = vim.api.nvim_buf_get_name(0)
	local home = os.getenv("HOME")
	abs_path = string.gsub(abs_path, home, "~")
	cfg.abs_path = abs_path
	M.file_type = vim.bo.filetype
	M.selected = get_selection()
end

local function create(type_name, subtype_name, filename, patterns)
	patterns = patterns or {}
	local cfg = require("note_search").config
	local tconf = cfg.types[type_name]
	local stconf = cfg.subtypes[subtype_name] or {}
	if not tconf then
		return vim.notify("Unknown type: " .. type_name, vim.log.levels.ERROR)
	end

	if vim.fn.filereadable(filename) == 1 then
		vim.cmd("edit " .. filename)
		return
	end

	local tpl_path = cfg.templates_dir .. "/"
	if stconf.template then
		tpl_path = tpl_path .. stconf.template
	else
		tpl_path = tpl_path .. tconf.template
	end
	patterns.type = type_name
	local content, err = template.render(tpl_path, patterns)
	if err then
		return vim.notify(err, vim.log.levels.ERROR)
	end

	local f = io.open(filename, "w")
	if f then
		f:write(content)
		f:close()
		vim.cmd("edit " .. filename)
		position_cursor("{{cursor}}")
		vim.cmd("startinsert")
	end
end

local function open_or_create(type_name, subtype_name, element)
	local cfg = require("note_search").config
	local tconf = cfg.types[type_name]
	local stconf = cfg.subtypes[subtype_name] or {}
	element = element or ""
	local name_of_note = ""
	if tconf.ask_for_name then
		local ok, result =
			pcall(vim.fn.inputdialog, "Title of " .. (subtype_name or "") .. " note for " .. element, "", "custom")
		if not ok then
			return
		end
		if not result or #result == 0 then
			return
		end
		name_of_note = result
	end
	local filename_of_note = string.lower(string.gsub(name_of_note, " ", "_"))
	local name = filename_of_note
	local simple_name = string.gsub(element, "_", " ")
	element = string.gsub(element, " ", "_")
	if stconf.day_prefix or tconf.day_prefix then
		local sep = #filename_of_note > 0 and "-" or ""
		filename_of_note = os_date_en(cfg.date_format) .. sep .. filename_of_note
		if #name_of_note == 0 then
			name_of_note = os_date_en(cfg.date_format)
		end
	end
	if stconf.add_type then
		filename_of_note = subtype_name .. "-" .. filename_of_note
	end
	local file_path = cfg.notes_dir .. "/" .. type_name .. "/"
	if element then
		file_path = file_path .. element .. "/"
	end
	if tconf.subdir then
		file_path = file_path .. os_date_en(tconf.subdir) .. "/"
	end
	vim.fn.mkdir(file_path, "p")
	file_path = file_path .. filename_of_note .. cfg.suffix
	if vim.fn.filereadable(file_path) == 1 then
		vim.cmd("edit " .. file_path)
		return
	end
	local replace_elements = { title = name_of_note, name = name, ref = element, refname = simple_name }
	create(type_name, subtype_name, file_path, replace_elements)
end

function M.note(type_name, subtype_name)
	local cfg = require("note_search").config
	save_original_position()
	if not type_name or #type_name == 0 then
		local options = {}
		for type in pairs(cfg.types) do
			table.insert(options, type)
		end
		Snacks.picker.select(options, { prompt = "Choose type", focus = "input" }, function(picked)
			if picked then
				M.create_note(picked, subtype_name)
			end
		end)
		return
	end
	M.create_note(type_name, subtype_name)
end

function M.create_note(type_name, subtype_name)
	local cfg = require("note_search").config
	local subtypes = require("note_search.subtypes")
	local tconf = cfg.types[type_name]
	if not tconf then
		return vim.notify("Unknown type: " .. type_name, vim.log.levels.ERROR)
	end
	local cwd = cfg.notes_dir .. "/" .. tconf.dir
	if not tconf.has_subtypes then
		open_or_create(type_name)
		return
	end
	Snacks.picker.files({
		cwd = cwd,
		title = "Search for instance of type " .. type_name,
		cmd = cfg.find_command,
		args = { "--max-depth", "1" },
		depth = 1,
		matcher = { fuzzy = true, frecency = true, history_bonus = true, ignore_case = true },
		file = { filename_first = true, git_status = false, truncate = "center" },
		actions = {
			confirm = function(picker, item)
				picker:close()
				local element = tostring(item.file):gsub(cfg.suffix, ""):lower()
				if subtype_name then
					open_or_create(type_name, subtype_name, element)
					return
				end
				local sub_elements = subtypes.list()
				Snacks.picker.select(sub_elements, { prompt = "Choose type", focus = "input" }, function(picked)
					if picked then
						open_or_create(type_name, picked, element)
					end
				end)
			end,
		},
	})
end

function M.create_type(type_name)
	local cfg = require("note_search").config
	local tconf = cfg.types[type_name]
	if not tconf then
		return vim.notify("Unknown type: " .. type_name, vim.log.levels.ERROR)
	end
	local cwd = cfg.notes_dir .. "/" .. tconf.dir

	Snacks.picker.files({
		cwd = cwd,
		title = "Search Notes (or type new name and create with Alt-e)",
		cmd = cfg.find_command,
		args = { "--max-depth", "1", "-i" },
		matcher = { fuzzy = false, frecency = true, history_bonus = true, ignore_case = true },
		file = { filename_first = true, git_status = false, truncate = "center" },
		win = { input = { keys = { ["<a-e>"] = { "create_new", mode = { "i", "n" } } } } },
		actions = {
			confirm = function(picker, item)
				local items = picker:items()
				if #items == 0 then
					local name_of_note = picker.finder.filter.pattern
					picker:close()
					local choice = vim.fn.confirm("Do you want to create " .. name_of_note .. "?", "&Yes\n&No", 2)
					if choice == 1 then
						local filepath = cfg.notes_dir .. "/" .. tconf.dir .. "/"
						if tconf.subdir then
							filepath = filepath .. os_date_en(tconf.subdir) .. "/"
						end
						local filename_of_note = string.lower(string.gsub(name_of_note, " ", "_"))
						if tconf.day_prefix then
							filename_of_note = os_date_en(cfg.date_format) .. "-"
						end
						filename_of_note = filename_of_note .. cfg.suffix
						local replace_elements = { title = name_of_note, name = filename_of_note }
						create(type_name, nil, filepath .. filename_of_note, replace_elements)
					end
					return
				end
				picker:close()
				if item then
					vim.api.nvim_command("edit " .. cwd .. "/" .. item.file)
				end
			end,
			create_new = function(picker)
				local content = picker.finder.filter.pattern
				picker:close()
				create(type_name, nil, content, {})
			end,
		},
	})
end

return M
