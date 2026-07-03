-- Markdown frontmatter updater for Neovim
-- On save, adds or updates the "updated" attribute in YAML frontmatter

local M = {}

-- Default format used for the "updated" attribute value
local updated_format = "%Y-%m-%d %H:%M:%S"

-- Detect YAML frontmatter in a list of lines.
-- Returns open_line, close_line (1-based, inclusive) or nil.
local function find_frontmatter(lines)
	if #lines < 3 then
		return nil
	end
	if lines[1] ~= "---" then
		return nil
	end
	for i = 2, #lines do
		if lines[i] == "---" then
			return 1, i
		end
	end
	return nil
end

-- Update the current buffer's frontmatter with the current date/time
-- as the "updated" attribute. No-op if the file is not markdown or has
-- no complete frontmatter section.
function M.update_on_save()
	local bufnr = vim.api.nvim_get_current_buf()
	local filename = vim.api.nvim_buf_get_name(bufnr)

	if not filename:match("%.md$") and not filename:match("%.markdown$") then
		return
	end

	local lines = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
	local open_line, close_line = find_frontmatter(lines)
	if not open_line or not close_line then
		return
	end

	local updated_line = "updated: " .. os.date(updated_format)

	-- Replace an existing "updated:" entry
	for i = open_line + 1, close_line - 1 do
		if lines[i]:match("^updated%s*:") then
			vim.api.nvim_buf_set_lines(bufnr, i - 1, i, false, { updated_line })
			return
		end
	end

	-- No existing entry: insert one just before the closing fence
	vim.api.nvim_buf_set_lines(bufnr, close_line - 1, close_line - 1, false, { updated_line })
end

-- Register the BufWritePre autocmd
function M.setup(opts)
	opts = opts or {}
	if opts.enabled == false then
		return
	end
	if opts.format then
		updated_format = opts.format
	end

	local group = vim.api.nvim_create_augroup("NoteSearchFrontmatter", { clear = true })
	vim.api.nvim_create_autocmd("BufWritePre", {
		group = group,
		pattern = { "*.md", "*.markdown" },
		callback = M.update_on_save,
	})
end

return M
