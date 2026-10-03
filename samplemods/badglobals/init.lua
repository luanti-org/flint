-- fine: luanti globals, the mod's own name, and a declared dependency
badglobals = {}

core.register_node("badglobals:stone", {
	description = "Bad Stone",
	tiles = { "default_stone.png" },
	sounds = default.node_sound_stone_defaults(),
})

-- bad: `farming` isn't in depends
core.register_craft({
	output = "badglobals:stone",
	recipe = { { farming.flour } },
})

-- bad: typo of `minetest`
minetset.log("action", "loaded badglobals")

-- bad: accidental global, should be local
counter = 0
function badglobals.count()
	counter = counter + 1
	return countr
end
