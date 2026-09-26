local modname=core.get_current_modname()
local modpath =   core.get_modpath(modname)
local S=core.get_translator(modname)
dofile(modpath.."/src/nodes.lua")
dofile(modpath .. '/src/jit.lua')

unformatted = {registered={},count=0,}
function unformatted.register(name,def) unformatted.registered[name]=def unformatted.count=unformatted.count+1 end

core.register_chatcommand("junk",{params="<text>",description=S("Say junk"),privs={shout=true},func=function(name,param)
  if param=="" then return false,S("Need text") end
      core.chat_send_all(name..": "..param)
  return true
end})

local t = {1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40}
local long_string_call = core.colorize("#ff0000", "this is a pretty long string argument") .. core.colorize("#00ff00", "and another long one")
local x = (((1+2)))
local s = 'single' .. "double" .. [[long
bracket]]
if not x then print("no") elseif x>2 then print( "big" ) else print("small") end
