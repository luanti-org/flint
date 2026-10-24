local S, PS = core.get_translator("generate")
local modpath = core.get_modpath("generate")

dofile(modpath .. "/src/items.lua")

-- settings: `generate.radius` is in settingtypes.txt, the rest get added
local radius = tonumber(core.settings:get("generate.radius")) or 8
local speed = tonumber(core.settings:get("generate.max_speed")) or 1.5
local count = tonumber(core.settings:get("generate_count")) or 3
local announce = core.settings:get_bool("generate.announce", true)
local greeting = core.settings:get("generate.greeting") or "hello"
-- needs 5.11, the newest thing used, so it decides min_minetest_version
local spawn = core.settings:get_pos("generate.spawn")
-- can't be generated, its possible flags aren't known
local flags = core.settings:get_flags("generate.flags")
-- another mod's setting, ignored
local other = core.settings:get("default.something")

-- optional dependencies, `default` is already a hard one
if core.get_modpath("default") and core.get_modpath("sfinv") then
	print("sfinv")
end
if core.get_modpath("mcl_core") then
	print("mineclone")
end

core.register_on_joinplayer(function(player)
	local name = player:get_player_name()
	if announce then
		-- TRANSLATORS: @1 is the player's name
		core.chat_send_player(name, S("Welcome, @1!", name))
	end
	-- needs 5.9
	core.log("action", core.sha256(name))
end)

core.register_chatcommand("generate_count", {
	description = S("Show how many things there are"),
	func = function()
		return true, PS("There is @1 thing.", "There are @1 things.", count, tostring(count))
	end,
})

print(radius, speed, greeting, spawn, flags, other)
