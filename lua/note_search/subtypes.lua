local M = {}

function M.list()
	local sconf = require("note_search").config.subtypes
	local result = {}
	for subtype in pairs(sconf) do
		table.insert(result, subtype)
	end
	return result
end

function M.get_sub_type(char)
	local sconf = require("note_search").config.subtypes
	for subtype in pairs(sconf) do
		if char == subtype:sub(1, 1) then
			return subtype
		end
	end
	return nil
end

return M
