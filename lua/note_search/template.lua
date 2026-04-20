local M = {}

local en_month_short = { "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec" }
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
	fmt = fmt:gsub("%%B", en_month_full[month_num]):gsub("%%b", en_month_short[month_num])
	return os.date(fmt, time)
end

function M.render(filepath, patterns, custom_time)
	patterns = patterns or {}
	local base_time = custom_time or os.time()
	local content
	local f = io.open(filepath, "r")
	if f then
		content = f:read("*a")
		f:close()
	else
		return nil, "Template not found: " .. filepath
	end
	local date = os_date_en("%A, %B %d, %Y", base_time)
	local time = os.date("%H:%M:%S", base_time)
	local today = os_date_en("%Y-%m-%d", base_time)
	local yesterday_time = base_time - 86400
	local tomorrow_time = base_time + 86400
	local yesterday = os_date_en("%Y-%m-%d", yesterday_time)
	local tomorrow = os_date_en("%Y-%m-%d", tomorrow_time)
	local month = os_date_en("%B", base_time)
	local year = os.date("%Y", base_time)
	-- Use plain string.gsub (not pattern matching) for template variables
	content = content
		:gsub("{{date}}", date)
		:gsub("{{time}}", time)
		:gsub("{{today}}", today)
		:gsub("{{yesterday}}", yesterday)
		:gsub("{{tomorrow}}", tomorrow)
		:gsub("{{month}}", month)
		:gsub("{{year}}", year)
		:gsub("{{title}}", patterns.title or "")
		:gsub("{{name}}", patterns.name or "")
		:gsub("{{type}}", patterns.type or "")
		:gsub("{{ref}}", patterns.ref or "")
		:gsub("{{refname}}", patterns.refname or "")
	return content
end

return M
