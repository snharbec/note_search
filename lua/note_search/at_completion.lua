-- @-triggered completion: typing `@` in insert mode opens a completion
-- menu of existing notes (ordered by modification time, newest first).
-- Picking a note replaces `@query` with `[[NoteName]]`.
--
-- The note list is built once via `note_search notes --list --sort modified`
-- and cached. The cache is rebuilt in the background after any markdown
-- BufWritePost, so completion stays responsive during typing.

local M = {}

-- Module state -----------------------------------------------------------
-- Cache: array of note-name strings, already sorted newest-first.
M.cache = {}
M.cache_loading = false
-- True while we're inside an `@`-completion session for the current
-- cursor position; used by the TextChangedI autocmd to keep the menu
-- in sync as the user types more characters after the `@`.
M.active = false
-- True while our popup is currently showing candidates; distinguishes
-- "we closed it ourselves after a zero-match query" (session stays
-- active) from "it closed because the user confirmed/dismissed it"
-- (session should end).
M.popup_open = false
-- True between `CompleteDonePre` and `CompleteDone`: a selection is
-- being confirmed (or discarded) and the resulting buffer edit's
-- `TextChanged{I,P}` event should be ignored rather than treated as
-- more typing or as the popup closing on its own.
M.confirming = false
-- 1-based column where the `@` that opened the session lives.
M.at_col = -1
-- Length (in chars) of the query typed so far after the `@`. A change
-- in this value triggers a re-fire of `complete()`.
M.query_len = 0

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

-- Locate the most recent `@` on the current line that:
--   - is preceded by a non-word character (or is at column 1), and
--   - whose trailing chars up to the cursor form a query of word chars
--     (letters/digits/_/-).
-- Returns (1-based `@` column, query length in chars) or nil.
local function current_at_token()
	local row, col0 = unpack(vim.api.nvim_win_get_cursor(0))
	local line = vim.api.nvim_buf_get_lines(0, row - 1, row, false)[1] or ""

	local best_at = nil
	-- Iterate in 1-based columns over the chars up to (and including)
	-- the current cursor position. `col0` is 0-based, so the last
	-- character the cursor sits on is at 1-based index `col0`.
	for at = 1, col0 do
		if line:sub(at, at) == "@" then
			local prev_ok = at == 1 or not line:sub(at - 1, at - 1):match("[%w_%-]")
			if prev_ok then
				best_at = at
			end
		end
	end
	if not best_at then
		return nil
	end

	-- Query is everything after the `@` up to the cursor.
	local query = line:sub(best_at + 1, col0)
	if query == "" or not query:match("^[%w_%-]*$") then
		-- Empty ("just typed @") or contains non-word chars. The empty
		-- case is still valid (show the full list); the non-word case
		-- means the user moved past the completion trigger (e.g. typed
		-- a space) and shouldn't be matched.
		if query == "" then
			return best_at, 0
		end
		return nil
	end
	return best_at, #query
end

-- Rebuild the completion menu from the cache, filtering against the
-- current query. Items preserve the cached order (newest first), so
-- the menu naturally surfaces recently-touched notes.
--
-- `at_col` is 1-based. `complete()` takes a 0-based byte start column;
-- we pass `at_col` (0-based) so nvim's default prefix matcher runs
-- against just the typed letters, not against the leading `@`.
local function fire_completion(at_col_1based)
	local row, col0 = unpack(vim.api.nvim_win_get_cursor(0))
	local line = vim.api.nvim_buf_get_lines(0, row - 1, row, false)[1] or ""
	local typed = line:sub(at_col_1based + 1, col0):lower()

	local items = {}
	for _, name in ipairs(M.cache) do
		if typed == "" or name:lower():find(typed, 1, true) then
			table.insert(items, {
				word = name,
				abbr = name,
				menu = "note",
				icase = 1,
				dup = 0,
			})
			if #items >= 500 then
				break
			end
		end
	end

	if #items == 0 then
		-- Close any open popup but keep the session active so the next
		-- keystroke can re-fire once the user keeps typing.
		if vim.fn.pumvisible() ~= 0 then
			vim.api.nvim_select_popupmenu_item(-1, false, true, {})
		end
		M.popup_open = false
		return false
	end

	-- Save and override `completeopt` for the duration of the session
	-- so the user's typical setting of auto-inserting the first match
	-- doesn't shadow our typed query. We only want our menu to surface
	-- candidates; the user confirms one explicitly with `<C-y>` or
	-- by clicking. The setting is restored when the session ends.
	if M.saved_completeopt == nil then
		M.saved_completeopt = vim.o.completeopt
	end
	vim.o.completeopt = "menu,noinsert,noselect,menuone"

	-- startcol is 0-based; passing `at_col_1based` (also the byte index
	-- for ASCII names) makes the matcher run against the typed query
	-- only. Items whose `word` doesn't share a prefix with the typed
	-- text get filtered out by nvim.
	vim.fn.complete(at_col_1based, items)
	M.popup_open = true
	return true
end

-- Restore the user's `completeopt` once the completion session ends.
-- Called from `on_complete_done` and from the bailout paths in
-- `on_text_changed_i`.
local function restore_completeopt()
	if M.saved_completeopt ~= nil then
		pcall(function()
			vim.o.completeopt = M.saved_completeopt
		end)
		M.saved_completeopt = nil
	end
end

-- Insert-mode `@` callback. With `expr = true` we return the literal
-- `@` string, and the schedule block fires the completion popup after
-- the cursor has been advanced past it.
local function on_at_typed()
	vim.schedule(function()
		local row, col0 = unpack(vim.api.nvim_win_get_cursor(0))
		local line = vim.api.nvim_buf_get_lines(0, row - 1, row, false)[1] or ""
		if col0 < 1 or line:sub(col0, col0) ~= "@" then
			return
		end
		-- Skip if the `@` is mid-word: typing `foo@` shouldn't trigger.
		local prev_ok = (col0 == 1) or not line:sub(col0 - 1, col0 - 1):match("[%w_%-]")
		if not prev_ok then
			return
		end

		M.active = true
		M.at_col = col0 -- already 1-based; cursor sits on the `@`
		M.query_len = 0
		fire_completion(M.at_col)
	end)
	return "@"
end

-- Marks that a selection is being confirmed or discarded, so the
-- `TextChanged{I,P}` fired by the resulting buffer edit doesn't get
-- mistaken for the user typing more or for the popup closing on its
-- own (both of which would otherwise end the session, or reopen the
-- popup, before `on_complete_done` runs below).
local function on_complete_done_pre()
	if not M.active then
		return
	end
	M.confirming = true
end

-- After `CompleteDone` the picked `word` has replaced the text from
-- `startcol` to the cursor. We then strip the leftover `@` and wrap
-- the bare name in `[[...]]`.
local function on_complete_done()
	if not M.active then
		return
	end
	local at_col_1 = M.at_col -- 1-based
	M.active = false
	M.popup_open = false
	M.confirming = false
	restore_completeopt()

	local row, col0 = unpack(vim.api.nvim_win_get_cursor(0))
	local line = vim.api.nvim_buf_get_lines(0, row - 1, row, false)[1] or ""
	-- startcol passed to `complete()` was `M.at_col` (1-based, the
	-- byte offset to match the leading `@`-position). nvim replaced
	-- chars [startcol, cursor] with the picked word, so the bare name
	-- now lives at 1-based columns [at_col_1 + 1, col0].
	local inserted = line:sub(at_col_1 + 1, col0)
	if inserted == "" or inserted:sub(1, 1) == "@" then
		return
	end
	if inserted:find("%[%[", 1, true) or inserted:find("%]%]", 1, true) then
		return
	end

	local new_text = "[[" .. inserted .. "]]"
	-- Replace from the `@` (0-based: at_col_1 - 1) through one past
	-- the last byte of the inserted word. `nvim_buf_set_text`'s
	-- end column is exclusive, so we pass `col0` (one past the last
	-- char) directly.
	vim.api.nvim_buf_set_text(0, row - 1, at_col_1 - 1, row - 1, col0, { new_text })

	local new_row, _ = unpack(vim.api.nvim_win_get_cursor(0))
	vim.api.nvim_win_set_cursor(0, { new_row, at_col_1 - 1 + #new_text })
end

-- Keep the popup in sync as the user keeps typing after the `@`. A
-- non-word character (space, slash, etc.) ends the session so we
-- don't shadow later edits.
local function on_text_changed_i()
	if not M.active then
		return
	end
	-- A selection is being confirmed or discarded: `on_complete_done`
	-- (fired next, via `CompleteDone`) owns cleanup and the text
	-- rewrite. Ignore the edit this triggered instead of treating it
	-- as more typing or as the popup closing on its own.
	if M.confirming then
		return
	end
	-- Bail if the popup was open and is now gone without us having
	-- closed it (and without a confirm/discard in progress): stale
	-- state, or the popup was dismissed some other way. If we closed
	-- it ourselves (zero-match query), `M.popup_open` is already
	-- false and the session stays active so a later backspace or a
	-- query that starts matching again can reopen it.
	if vim.fn.pumvisible() == 0 and M.popup_open then
		M.active = false
		M.popup_open = false
		restore_completeopt()
		return
	end
	local at_col_1, query_len = current_at_token()
	if not at_col_1 then
		M.active = false
		M.popup_open = false
		if vim.fn.pumvisible() ~= 0 then
			vim.api.nvim_select_popupmenu_item(-1, false, true, {})
		end
		restore_completeopt()
		return
	end

	local row, col0 = unpack(vim.api.nvim_win_get_cursor(0))
	local line = vim.api.nvim_buf_get_lines(0, row - 1, row, false)[1] or ""
	local last = line:sub(col0, col0)
	if last ~= "" and not last:match("[%w_%-@]") then
		M.active = false
		M.popup_open = false
		if vim.fn.pumvisible() ~= 0 then
			vim.api.nvim_select_popupmenu_item(-1, false, true, {})
		end
		restore_completeopt()
		return
	end

	if query_len ~= M.query_len or at_col_1 ~= M.at_col then
		M.query_len = query_len
		M.at_col = at_col_1
		fire_completion(at_col_1)
	end
end

-- Public API -----------------------------------------------------------

function M.setup(opts)
	opts = opts or {}
	if opts.note_search_cmd then
		M.note_search_cmd = opts.note_search_cmd
	end

	local group = vim.api.nvim_create_augroup("NoteSearchAtCompletion", { clear = true })

	-- Install the buffer-local insert-mode `@` keymap every time a
	-- markdown buffer comes into scope. Buffer-local mappings are
	-- scoped to that buffer, so the global `@` mapping in non-markdown
	-- buffers (e.g. when typing email addresses or shell commands) is
	-- left alone.
	vim.api.nvim_create_autocmd("FileType", {
		group = group,
		pattern = { "markdown" },
		callback = function(args)
			vim.keymap.set(
				"i",
				"@",
				on_at_typed,
				{ expr = true, noremap = true, silent = true, buffer = args.buf }
			)
		end,
	})

	vim.api.nvim_create_autocmd({ "TextChangedI", "TextChangedP" }, {
		group = group,
		callback = on_text_changed_i,
	})
	vim.api.nvim_create_autocmd("CompleteDonePre", {
		group = group,
		callback = on_complete_done_pre,
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
	-- keystroke re-fires `complete()` and the menu fills in.
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
