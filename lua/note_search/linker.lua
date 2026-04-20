local M = {}

function M.get_backlinks(name)
	local cfg = require("note_search").config
	local result = {}
	local handles = io.popen(cfg.find_command .. " -g '*.md' " .. cfg.notes_dir .. " | head -100")
	if handles then
		for file in handles:lines() do
			local f = io.open(file, "r")
			if f then
				for line in f:lines() do
					if string.find(line, "%[%" .. name .. "%]") or string.find(line, "\\[" .. name .. "\\]") then
						table.insert(result, file)
						break
					end
				end
				f:close()
			end
		end
		handles:close()
	end
	return result
end

return M
