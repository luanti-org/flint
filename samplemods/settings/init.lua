-- fine: in settingtypes.txt
local speed = tonumber(core.settings:get("settings.speed")) or 1.5
local enabled = core.settings:get_bool("settings_enabled", true)

-- fine: engine and other mods' settings
local damage = core.settings:get_bool("enable_damage")
local other = core.settings:get("default.something")

-- bad: our own settings, missing from settingtypes.txt
local radius = tonumber(core.settings:get("settings.radius")) or 10
local spawn = core.settings:get_pos("settings_spawn")

print(speed, enabled, damage, other, radius, spawn)
