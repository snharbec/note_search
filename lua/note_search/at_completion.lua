-- @-triggered completion: typing `@` in insert mode opens a completion
-- menu of existing notes (ordered by modification time, newest first),
-- filtered to names containing the typed text anywhere (not just as a
-- prefix). Picking a note replaces `@query` with `[[NoteName]]`.
--
-- Implemented as a 'completefunc' (see `:h complete-functions`) invoked
-- via `i_CTRL-X_CTRL-U`, rather than driving `vim.fn.complete()` and the
-- popup by hand: with `refresh = "always"` nvim re-invokes the match
-- phase itself on every keystroke, which is the same mechanism LSP
-- omnifunc completion relies on for live, non-prefix filtering. That
-- sidesteps a family of bugs in the hand-rolled version (nvim's own
-- internal re-filtering of a manually supplied candidate list not
-- reliably reflecting our matches, `pumvisible()` reading transiently
-- false mid-refilter, and stray `TextChanged*` events fired by the
-- popup's own live preview while navigating with <C-n>/<C-p>).
--
-- The note list is built once via `note_search notes --list --sort modified`
-- and cached. The cache is rebuilt in the background after any markdown
-- BufWritePost, so completion stays responsive during typing.

local M = {}

-- Module state -----------------------------------------------------------
-- Cache: array of note-name strings, already sorted newest-first.
M.cache = {}
M.cache_loading = false

-- Config ---------------------------------------------------------------
-- We read `note_search_cmd` from `note_search.config` at setup time and
-- fall back to plain "note_search" on $PATH.
M.note_search_cmd = "note_search"

-- Utilities ------------------------------------------------------------

-- Strip the "[N todos, M links]" tail and ".md" extension from a
-- `note_search notes --list` line to recover the bare note name.
local function parse_note_name(line)
	local path = line:match("^(.-)%s*%[%d+%s+todos")
		or line:match("^(.-)%s*%[")
	if not path or path == "" then
		return nil
	end
	path = path:gsub("^%s+", ""):gsub("%s+$", "")
	if path == "" then
		return nil
	end
	local basename = path:match("([^/]+)$") or path
	basename = basename:gsub("%.md$", ""):gsub("%.markdown$", "")
	if basename == "" then
		return nil
	end
	return basename
end

-- Run `note_search notes --list --sort modified --absolute-path` and
-- rebuild M.cache. Long-running; meant to be called via vim.schedule.
local function refresh_cache()
	if M.cache_loading then
		return
	end
	M.cache_loading = true

	local cmd = {
		M.note_search_cmd,
		"notes",
		"--list",
		"--sort",
		"modified",
		"--absolute-path",
	}

	vim.system(cmd, { text = true }, function(result)
		vim.schedule(function()
			M.cache_loading = false
			if result.code ~= 0 then
				vim.notify(
					"note_search failed (at_completion cache): " .. (result.stderr or ""),
					vim.log.levels.WARN
				)
				return
			end

			local seen = {}
			local names = {}
			for line in (result.stdout or ""):gmatch("[^\r\n]+") do
				local name = parse_note_name(line)
				if name and not seen[name] then
					seen[name] = true
					table.insert(names, name)
				end
			end
			M.cache = names
		end)
	end)
end

-- Scan backward from the cursor (0-based `col0`) through query
-- characters (word chars) for the `@` that starts this link token.
-- Returns its 1-based column, which (by a convenient coincidence of
-- 0-based/1-based arithmetic) is also the 0-based startcol `complete()`
-- and `completefunc` expect: the byte offset right after the `@`.
local function find_at_col(line, col0)
	for at = col0, 1, -1 do
		local c = line:sub(at, at)
		if c == "@" then
			local prev_ok = at == 1 or not line:sub(at - 1, at - 1):match("[%w_%-]")
			if prev_ok then
				return at
			end
			return nil
		elseif not c:match("[%w_%-]") then
			return nil
		end
	end
	return nil
end

-- `completefunc` entry point (see `:h complete-functions`). Despite
-- `:h complete-functions` describing findstart/match as a "first
-- call"/"later calls" pair, nvim calls findstart again on every
-- `refresh = "always"` re-invocation too (not just the match phase),
-- so it must keep finding the `@` as the query grows rather than only
-- recognizing the cursor position from the moment completion started.
function M.complete_func(findstart, base)
	if findstart == 1 then
		local row, col0 = unpack(vim.api.nvim_win_get_cursor(0))
		local line = vim.api.nvim_buf_get_lines(0, row - 1, row, false)[1] or ""
		local at_col = find_at_col(line, col0)
		if not at_col then
			return -3 -- cancel silently, leave completion mode
		end
		return at_col
	end

	local typed = base:lower()
	local items = {}
	for _, name in ipairs(M.cache) do
		if typed == "" or name:lower():find(typed, 1, true) then
			table.insert(items, { word = name, abbr = name, menu = "note" })
			if #items >= 500 then
				break
			end
		end
	end
	-- `refresh = "always"` makes nvim call us again (match phase only)
	-- on every keystroke instead of narrowing the returned list itself
	-- with its own (prefix-only) matcher.
	return { words = items, refresh = "always" }
end

local ctrl_x_ctrl_u = vim.api.nvim_replace_termcodes("<C-x><C-u>", true, false, true)

-- Insert-mode `@` callback. With `expr = true` the returned string is
-- inserted as if typed: the `@` itself, followed by the keys that
-- trigger `completefunc`-based completion (skipped when `@` would be
-- mid-word, e.g. typing `foo@`).
local function on_at_typed()
	local row, col0 = unpack(vim.api.nvim_win_get_cursor(0))
	local line = vim.api.nvim_buf_get_lines(0, row - 1, row, false)[1] or ""
	local prev_ok = (col0 == 0) or not line:sub(col0, col0):match("[%w_%-]")
	if not prev_ok then
		return "@"
	end
	return "@" .. ctrl_x_ctrl_u
end

-- After completion is confirmed, replace the bare inserted note name
-- with a `[[...]]` link. Guarded by checking that an `@` actually
-- precedes the inserted text, since `CompleteDone` fires for any
-- Insert-mode completion in any buffer, not just ours.
local function on_complete_done()
	local completed = vim.v.completed_item
	local word = completed and completed.word
	if not word or word == "" then
		return
	end

	local row, col0 = unpack(vim.api.nvim_win_get_cursor(0))
	local line = vim.api.nvim_buf_get_lines(0, row - 1, row, false)[1] or ""
	-- Cursor sits right after the inserted word; the `@` should be the
	-- byte immediately before it.
	local word_start_0 = col0 - #word -- 0-based
	if word_start_0 < 1 or line:sub(word_start_0, word_start_0) ~= "@" then
		return
	end
	if word:find("%[%[", 1, true) or word:find("%]%]", 1, true) then
		return
	end

	local new_text = "[[" .. word .. "]]"
	vim.api.nvim_buf_set_text(0, row - 1, word_start_0 - 1, row - 1, col0, { new_text })
	vim.api.nvim_win_set_cursor(0, { row, word_start_0 - 1 + #new_text })
end

-- Public API -----------------------------------------------------------

function M.setup(opts)
	opts = opts or {}
	if opts.note_search_cmd then
		M.note_search_cmd = opts.note_search_cmd
	end

	local group = vim.api.nvim_create_augroup("NoteSearchAtCompletion", { clear = true })

	-- Install the buffer-local insert-mode `@` keymap (and completefunc)
	-- every time a markdown buffer comes into scope. Buffer-local
	-- mappings are scoped to that buffer, so the global `@` mapping in
	-- non-markdown buffers (e.g. when typing email addresses or shell
	-- commands) is left alone.
	vim.api.nvim_create_autocmd("FileType", {
		group = group,
		pattern = { "markdown" },
		callback = function(args)
			vim.bo[args.buf].completefunc = "v:lua.require'note_search.at_completion'.complete_func"
			-- `noinsert,noselect`: nothing is inserted or selected
			-- until the user explicitly confirms an item, so typing
			-- more of the query doesn't get shadowed by a preview
			-- of the first candidate.
			vim.opt_local.completeopt = { "menu", "noinsert", "noselect", "menuone" }
			vim.keymap.set(
				"i",
				"@",
				on_at_typed,
				{ expr = true, noremap = true, silent = true, buffer = args.buf }
			)
		end,
	})

	vim.api.nvim_create_autocmd("CompleteDone", {
		group = group,
		callback = on_complete_done,
	})

	-- Refresh the cache lazily on the first markdown buffer entry and
	-- after every markdown save (debounced to a single scheduled call).
	vim.api.nvim_create_autocmd({ "BufEnter", "BufWritePost" }, {
		group = group,
		pattern = { "*.md", "*.markdown" },
		callback = function(args)
			if args.event == "BufWritePost" then
				if not M._refresh_pending then
					M._refresh_pending = true
					vim.schedule(function()
						M._refresh_pending = false
						refresh_cache()
					end)
				end
			elseif #M.cache == 0 then
				refresh_cache()
			end
		end,
	})

	-- Kick off an initial load in the background. On the very first
	-- `@` before the cache arrives the popup is empty; the next
	-- keystroke re-fires the match phase and the menu fills in.
	if #M.cache == 0 then
		vim.schedule(refresh_cache)
	end
end

-- Force a refresh. Mostly useful from tests; normal flows rely on the
-- BufWritePost-driven background refresh.
function M.refresh()
	refresh_cache()
end

return M
