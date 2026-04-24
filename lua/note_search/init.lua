local M = {}

M.defaults = {
	notes_dir = vim.fn.expand(vim.env.NOTE_SEARCH_DIR or "~/.local/share/notes"),
	templates_dir = vim.fn.expand(vim.env.NOTE_SEARCH_DIR or "~/.local/share/notes") .. "/templates",
	types = {
		daily = {
			name = "daily",
			dir = "daily",
			subdir = "%Y/%b",
			template = "daily.md",
			day_prefix = true,
			create = "t",
			link = "t",
		},
		note = {
			name = "note",
			dir = "note",
			template = "note.md",
			ask_for_name = true,
			create = "n",
			link = "n",
		},
		person = {
			name = "person",
			dir = "person",
			ask_for_name = true,
			has_subtypes = true,
			template = "person.md",
			create = "E",
			note = "e",
			link = "e",
		},
		project = {
			name = "project",
			dir = "project",
			ask_for_name = true,
			has_subtypes = true,
			template = "project.md",
			create = "P",
			note = "p",
			link = "p",
		},
		company = {
			name = "company",
			dir = "company",
			ask_for_name = true,
			has_subtypes = true,
			template = "company.md",
			create = "C",
			note = "c",
			link = "c",
		},
		department = {
			name = "department",
			dir = "department",
			ask_for_name = true,
			has_subtypes = true,
			template = "department.md",
			create = "D",
			note = "d",
			link = "d",
		},
	},
	subtypes = {
		Meeting = { template = "meeting.md", add_type = true, day_prefix = true },
		Note = { template = "note.md", add_type = true, day_prefix = true },
		Task = { template = "task.md", add_type = true, day_prefix = true },
	},
	date_format = "%Y-%m-%d",
	keymap_group = "<leader>n",
	insert_group = ".",
	suffix = ".md",
	find_command = "fd",
	-- Markdown exec blocks configuration
	exec = {
		auto_execute = false,         -- Auto-run exec blocks on buffer load
		output_marker = "<!-- exec-output -->",
	},
}

function M.setup(opts)
	M.config = vim.tbl_deep_extend("force", M.defaults, opts or {})
	require("note_search.commands").setup(M.config)
	-- Setup markdown exec processing with user's exec config
	require("note_search.markdown-exec").setup(M.config.exec)
end

return M
