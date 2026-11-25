-- SPDX-License-Identifier: GPL-3.0-or-later
--
-- What a plug-in's script sees: `plugin`, `param`, `ui` and its widgets, `presets`, and the
-- `g` its `build` is handed. Each only writes down what it was told, in the table `__alpaca`,
-- which the host reads once the script has run. No sample goes through Lua.

-- a script gets no way to load more code
dofile, loadfile, load, loadstring, require, collectgarbage = nil, nil, nil, nil, nil, nil

local A = { params = {}, presets = {}, nodes = {}, outputs = {}, param_node = {} }
__alpaca = A

function plugin(t) A.plugin = t end

local function declare(kind)
  return function(t)
    if type(t) ~= "table" then error("param." .. kind .. " takes a table", 2) end
    t.kind = kind
    A.params[#A.params + 1] = t
    return { __p = #A.params }
  end
end

param = { number = declare("number"), choice = declare("choice"), toggle = declare("toggle") }

function presets(t) A.presets = t end

-- the layout: written down, with the kind of every widget
function ui(t) A.ui = t end
function slider(p, opts) return { widget = "slider", param = p, opts = opts } end
function toggle(p, opts) return { widget = "toggle", param = p, opts = opts } end
function choice(p, opts) return { widget = "choice", param = p, opts = opts } end
function label(text, opts) return { widget = "label", text = text, opts = opts } end
function group(name, items, opts) return { widget = "group", name = name, items = items, opts = opts } end

-- the graph

local function node(prim, edges, consts)
  A.nodes[#A.nodes + 1] = { prim = prim, edges = edges or {}, consts = consts or {} }
  return { __n = #A.nodes - 1 }
end

-- what a node reads: a number, another node, or a parameter, which is a node made once
local function edge(v, level)
  if type(v) == "number" then return { c = v } end
  if type(v) == "boolean" then return { c = v and 1 or 0 } end
  if type(v) == "table" then
    if v.__n then return { n = v.__n } end
    if v.__p then
      if not A.param_node[v.__p] then
        A.param_node[v.__p] = node("param", {}, { index = v.__p - 1 }).__n
      end
      return { n = A.param_node[v.__p] }
    end
  end
  error("bad argument: a number, a node or a parameter expected, got " .. type(v), (level or 2) + 1)
end

-- which options of a primitive take a node; the rest are fixed when the graph is built
local EDGES = {
  gain = { input = true, db = true },
  math = { input = true },
  biquad = { input = true, freq = true, q = true, gainDb = true },
  onepole = { input = true, coeff = true },
  allpass1 = { input = true, coeff = true },
  delay = { input = true, time = true },
  tap = { time = true },
  lfo = { rate = true, phase = true },
  envelope = { input = true },
  softclip = { input = true, drive = true },
  clip = { input = true, min = true, max = true },
  noise = {},
}

-- a primitive that takes its options in one table
local function with_options(prim)
  return function(t)
    if t == nil then t = {} end
    if type(t) ~= "table" or t.__n or t.__p then
      error("g." .. prim .. " takes its options in a table", 2)
    end
    local edges, consts = {}, {}
    for key, value in pairs(t) do
      if EDGES[prim][key] then
        edges[key] = edge(value, 2)
      elseif prim == "tap" and key == "line" then
        if type(value) ~= "table" or not value.__n then
          error("g.tap: line is the handle a g.delay returned, and nothing else", 2)
        end
        consts[key] = value.__n
      elseif type(value) == "number" or type(value) == "string" then
        consts[key] = value
      else
        error("bad argument: " .. key .. " of g." .. prim .. " is fixed when the graph is built: " ..
          "a number or a word expected, got " .. type(value), 2)
      end
    end
    return node(prim, edges, consts)
  end
end

local function two(prim)
  return function(a, b) return node(prim, { a = edge(a), b = edge(b) }) end
end

-- two or more terms, folded left to right
local function many(prim)
  return function(...)
    local terms = { ... }
    if #terms < 2 then error("g." .. prim .. " takes two or more terms", 2) end
    local sum = terms[1]
    for i = 2, #terms do sum = node(prim, { a = edge(sum), b = edge(terms[i]) }) end
    return sum
  end
end

local function name_of(name, what)
  if type(name) ~= "string" then error("g." .. what .. ": a feedback name is a word", 3) end
  return name
end

local g = {
  input = function(ch)
    if type(ch) ~= "number" then error("g.input takes a channel number", 2) end
    return node("input", {}, { ch = ch })
  end,
  add = many("add"), mul = many("mul"),
  sub = two("sub"), div = two("div"), min = two("min"), max = two("max"),
  crossfade = function(a, b, t) return node("crossfade", { a = edge(a), b = edge(b), t = edge(t) }) end,
  sanitise = function(input) return node("sanitise", { input = edge(input) }) end,
  tapIn = function(name, source)
    node("tapIn", { source = edge(source) }, { name = name_of(name, "tapIn") })
  end,
  tapOut = function(name) return node("tapOut", {}, { name = name_of(name, "tapOut") }) end,
}
for prim in pairs(EDGES) do g[prim] = with_options(prim) end

setmetatable(g, { __index = function(_, name)
  error("g." .. tostring(name) .. " is not a primitive", 2)
end })

-- runs the script's build for one audio format and writes down what each channel leaves by
function __alpaca_build(sample_rate, channels)
  if type(build) ~= "function" then error("the plug-in has no build function", 0) end
  local out = build(g, { sampleRate = sample_rate, channels = channels })
  if type(out) ~= "table" then error("build returns a table with one node for each channel", 0) end
  for ch = 0, channels - 1 do
    if out[ch] == nil then error("build returned nothing for channel " .. ch, 0) end
    A.outputs[ch + 1] = edge(out[ch], 1)
  end
end
