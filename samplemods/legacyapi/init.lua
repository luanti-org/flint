core.register_on_joinplayer(function(player)
	-- fine: table form
	player:set_physics_override({ speed = 1.5 })

	-- bad: old positional form, speed, jump, gravity
	player:set_physics_override(1.5, 1, 0.5)

	-- bad: nil left a value unchanged
	player:set_physics_override(nil, 2)
end)

-- flint: allow(luanti_legacy_physics_override)
core.get_player_by_name("singleplayer"):set_physics_override(1, 1, 1)

-- bad: deprecated env, on its own and with a method after it
local env = core.env
local node = minetest.env:get_node({ x = 0, y = 0, z = 0 })
print(env, node)

-- bad: minetest instead of core
minetest.log("action", "loaded legacyapi")

-- bad: deprecated functions
core.register_on_auth_fail(function(_name, _ip) end)
local spawn = core.settings and core.setting_get_pos("static_spawnpoint")
local noise = core.get_perlin(0, 3, 0.5, 100)
local old_noise = core.get_value_noise(0, 3, 0.5, 100)
local params = core.get_mapgen_params()
local group = core.get_node_group("default:stone", "cracky")
print(spawn, noise, old_noise, params, group)

-- bad: deprecated methods
core.register_on_joinplayer(function(player)
	player:add_player_velocity(player:get_player_velocity())
	player:set_look_yaw(player:get_look_yaw())
	player:set_look_pitch(player:get_look_pitch())
end)
core.register_on_punchplayer(function(_, hitter)
	print(hitter:get_entity_name())
end)
