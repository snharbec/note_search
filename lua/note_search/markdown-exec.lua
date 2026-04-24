-- Markdown Exec Block Processor for Neovim
-- Processes code blocks marked with 'exec' in markdown files
-- Automatically executes on load (configurable) and cleans up on save

local M = {}

-- Configuration
local config = {
  -- Pattern to match exec code blocks
  -- Matches: ```exec, ```bash exec, ```sh exec, etc.
  block_start_pattern = "^```(%w*)%s*exec",
  block_end_pattern = "^```$",
  -- Marker to identify generated content (for cleanup)
  output_marker = "<!-- exec-output -->",
  -- Auto-execute exec blocks on buffer load (default: true)
  auto_execute = true,
}

-- Get the current note name without .md suffix
local function get_note_name()
  local filename = vim.api.nvim_buf_get_name(vim.api.nvim_get_current_buf())
  -- Extract basename and remove .md extension
  local basename = filename:match("([^/]+)$") or filename
  return basename:gsub("%.md$", ""):gsub("%.markdown$", "")
end

-- Get today's date in YYYY-MM-DD format
local function get_today()
  return os.date("%Y-%m-%d")
end

-- Expand patterns in command string
local function expand_patterns(cmd)
  if not cmd then return cmd end
  -- Replace {{note}} with current note name (without .md)
  cmd = cmd:gsub("{{note}}", get_note_name())
  -- Replace {{today}} with current date YYYY-MM-DD
  cmd = cmd:gsub("{{today}}", get_today())
  return cmd
end

-- Find the last heading level (number of # characters) before a given line
local function get_last_heading_level(lines, before_line)
  local level = 0
  for i = 1, before_line - 1 do
    local line = lines[i]
    -- Match markdown headings: one or more # at start of line followed by space
    local hashes = line:match("^(#+) ")
    if hashes then
      level = #hashes
    end
  end
  return level
end

-- Adjust heading levels in output by adding prefix_hashes to each heading
local function adjust_heading_levels(output_lines, prefix_hashes)
  if prefix_hashes <= 0 then
    return output_lines
  end

  local adjusted = {}
  local prefix = string.rep("#", prefix_hashes)

  for _, line in ipairs(output_lines) do
    -- Check if line is a heading (starts with # followed by space)
    local hashes, content = line:match("^(#+) (.*)")
    if hashes and content then
      -- Prepend the prefix hashes
      table.insert(adjusted, prefix .. line)
    else
      table.insert(adjusted, line)
    end
  end

  return adjusted
end

-- Execute a command and return output lines
local function execute_command(cmd)
  local output = {}
  local handle = io.popen(cmd .. " 2>&1")
  if handle then
    for line in handle:lines() do
      table.insert(output, line)
    end
    handle:close()
  end
  return output
end

-- Find all exec code blocks in buffer
local function find_exec_blocks(bufnr)
  local lines = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
  local blocks = {}
  local in_block = false
  local block_start = nil
  local block_lang = nil
  local first_line = nil

  for i, line in ipairs(lines) do
    local lang = line:match(config.block_start_pattern)
    if lang then
      in_block = true
      block_start = i
      block_lang = lang ~= "" and lang or "sh"
      first_line = nil
    elseif in_block and line:match(config.block_end_pattern) then
      in_block = false
      if first_line then
        table.insert(blocks, {
          start_line = block_start,
          end_line = i,
          lang = block_lang,
          command = first_line,
          output_start = nil, -- Will be set if output exists
        })
      end
    elseif in_block and first_line == nil then
      -- This is the first line after the opening ```
      -- Skip empty lines to find the actual command
      if line:match("^%s*$") == nil then
        first_line = line
      end
    end
  end

  return lines, blocks
end

-- Process exec blocks on file load
function M.process_on_load()
  print("process_on_load")
  local bufnr = vim.api.nvim_get_current_buf()
  local filename = vim.api.nvim_buf_get_name(bufnr)

  -- Only process markdown files
  if not filename:match("%.md$") and not filename:match("%.markdown$") then
    return
  end

  local lines, blocks = find_exec_blocks(bufnr)
  if #blocks == 0 then
    return
  end

  -- Process from bottom to top to avoid line number shifts
  for i = #blocks, 1, -1 do
    local block = blocks[i]

    -- Check if already has output marker
    local has_output = false
    for j = block.start_line + 1, block.end_line - 1 do
      if lines[j] and lines[j]:find(config.output_marker, 1, true) then
        has_output = true
        break
      end
    end

    -- Skip if already has output
    if not has_output then
      -- Expand patterns and execute the first line
      local expanded_cmd = expand_patterns(block.command)
      local output = execute_command(expanded_cmd)

      -- Find the last heading level before this exec block
      local heading_level = get_last_heading_level(lines, block.start_line)

      -- Adjust heading levels in output
      if heading_level > 0 then
        output = adjust_heading_levels(output, heading_level)
      end

      -- Prepare output lines with marker
      local output_lines = { config.output_marker }
      for _, out_line in ipairs(output) do
        table.insert(output_lines, out_line)
      end

      -- Insert after the first command line (which is at block.start_line + 1)
      -- Find the actual position (skip empty lines after ```)
      local insert_line = block.start_line + 1
      while lines[insert_line] and lines[insert_line]:match("^%s*$") do
        insert_line = insert_line + 1
      end

      vim.api.nvim_buf_set_lines(bufnr, insert_line, insert_line, false, output_lines)
    end
  end
end

-- Clean up exec output before saving
function M.cleanup_on_save()
  local bufnr = vim.api.nvim_get_current_buf()
  local filename = vim.api.nvim_buf_get_name(bufnr)

  -- Only process markdown files
  if not filename:match("%.md$") and not filename:match("%.markdown$") then
    return
  end

  -- Store that we've modified this buffer for restoration
  vim.b.markdown_exec_modified = true

  local lines = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
  local in_exec_block = false
  local exec_block_start = nil
  local lines_to_remove = {}

  for i, line in ipairs(lines) do
    if line:match(config.block_start_pattern) then
      in_exec_block = true
      exec_block_start = i
    elseif in_exec_block and line:match(config.block_end_pattern) then
      in_exec_block = false
    elseif in_exec_block and line:find(config.output_marker, 1, true) then
      -- Found output marker, remove from here to end of block (but before ```)
      table.insert(lines_to_remove, i)
      -- Also mark subsequent lines until we hit ``` or another block
      local j = i + 1
      while j <= #lines and not lines[j]:match(config.block_end_pattern) do
        table.insert(lines_to_remove, j)
        j = j + 1
      end
    end
  end

  -- Remove from bottom to top
  table.sort(lines_to_remove, function(a, b) return a > b end)
  for _, line_num in ipairs(lines_to_remove) do
    vim.api.nvim_buf_set_lines(bufnr, line_num - 1, line_num, false, {})
  end
end

-- Restore exec output after save (optional - if you want to see it again)
function M.restore_after_save()
  if vim.b.markdown_exec_modified then
    vim.b.markdown_exec_modified = false
    -- Optionally re-process to show output again
    -- Uncomment the next line if you want auto-restore:
    -- M.process_on_load()
  end
end

-- Manual trigger to process exec blocks
function M.process_now()
  M.process_on_load()
  print("Markdown exec blocks processed")
end

-- Toggle exec block execution (execute if no output, remove if output exists)
function M.toggle()
  local bufnr = vim.api.nvim_get_current_buf()
  local filename = vim.api.nvim_buf_get_name(bufnr)

  -- Only process markdown files
  if not filename:match("%.md$") and not filename:match("%.markdown$") then
    print("Not a markdown file")
    return
  end

  local lines, blocks = find_exec_blocks(bufnr)
  if #blocks == 0 then
    print("No exec blocks found")
    return
  end

  -- Check if any block has output
  local has_any_output = false
  for _, block in ipairs(blocks) do
    for j = block.start_line + 1, block.end_line - 1 do
      if lines[j] and lines[j]:find(config.output_marker, 1, true) then
        has_any_output = true
        break
      end
    end
    if has_any_output then break end
  end

  if has_any_output then
    -- Remove output
    M.cleanup_output()
    print("Exec output removed")
  else
    -- Execute and show output
    M.process_now()
  end
end

-- Clean up exec output (for toggle command)
function M.cleanup_output()
  local bufnr = vim.api.nvim_get_current_buf()
  local filename = vim.api.nvim_buf_get_name(bufnr)

  -- Only process markdown files
  if not filename:match("%.md$") and not filename:match("%.markdown$") then
    return
  end

  local lines = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
  local in_exec_block = false
  local lines_to_remove = {}

  for i, line in ipairs(lines) do
    if line:match(config.block_start_pattern) then
      in_exec_block = true
    elseif in_exec_block and line:match(config.block_end_pattern) then
      in_exec_block = false
    elseif in_exec_block and line:find(config.output_marker, 1, true) then
      -- Found output marker, remove from here to end of block (but before ```)
      table.insert(lines_to_remove, i)
      -- Also mark subsequent lines until we hit ``` or another block
      local j = i + 1
      while j <= #lines and not lines[j]:match(config.block_end_pattern) do
        table.insert(lines_to_remove, j)
        j = j + 1
      end
    end
  end

  -- Remove from bottom to top
  table.sort(lines_to_remove, function(a, b) return a > b end)
  for _, line_num in ipairs(lines_to_remove) do
    vim.api.nvim_buf_set_lines(bufnr, line_num - 1, line_num, false, {})
  end
end

-- Setup autocommands
function M.setup(opts)
  opts = opts or {}
  if opts.block_start_pattern then
    config.block_start_pattern = opts.block_start_pattern
  end
  if opts.output_marker then
    config.output_marker = opts.output_marker
  end
  if opts.auto_execute ~= nil then
    config.auto_execute = opts.auto_execute
  end

  local group = vim.api.nvim_create_augroup("MarkdownExec", { clear = true })

  -- Process on load (only if auto_execute is enabled)
  if config.auto_execute then
    vim.api.nvim_create_autocmd({ "BufReadPost", "BufNewFile" }, {
      group = group,
      pattern = { "*.md", "*.markdown" },
      callback = M.process_on_load,
    })
  end

  -- Cleanup before save (always enabled)
  vim.api.nvim_create_autocmd("BufWritePre", {
    group = group,
    pattern = { "*.md", "*.markdown" },
    callback = M.cleanup_on_save,
  })

  -- Optional: Restore after save (only if auto_execute is enabled)
  if config.auto_execute then
    vim.api.nvim_create_autocmd("BufWritePost", {
      group = group,
      pattern = { "*.md", "*.markdown" },
      callback = M.restore_after_save,
    })
  end
end

-- Create user command
vim.api.nvim_create_user_command("MarkdownExecProcess", function()
  M.process_now()
end, { desc = "Process markdown exec blocks" })

-- Create user command to toggle exec output
vim.api.nvim_create_user_command("MarkdownExecToggle", function()
  M.toggle()
end, { desc = "Toggle markdown exec block execution/output" })

-- Create user command to toggle auto-processing
vim.api.nvim_create_user_command("MarkdownExecSetup", function()
  M.setup()
  print("Markdown exec auto-processing enabled")
end, { desc = "Setup markdown exec autocmds" })

return M
