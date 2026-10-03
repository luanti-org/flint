---
title: Lint
---

calls [selene](https://github.com/Kampfkarren/selene)

* supports luanti globals by default
* allows calling a globals that are the name of mods dep'd on
* configured by `[linter]` in [flint.toml](config.md)

## Luanti lints

flint adds its own lints on top of selene's. they're configured the same way, in `[linter.lints]` and `[linter.config]`.

| lint | default | description |
| --- | --- | --- |
| `luanti_legacy_physics_override` | deny | `player:set_physics_override(speed, jump, gravity)` instead of a table, e.g. `player:set_physics_override({ speed = 1.5 })` |
| `luanti_deprecated_env` | deny | `core.env` or `minetest.env`, on its own or with anything after it. its methods are functions on `core`, e.g. `core.get_node(pos)` instead of `core.env:get_node(pos)` |
| `luanti_minetest_global` | warn | the `minetest` global instead of `core`, e.g. `core.log("hi")` instead of `minetest.log("hi")` |
| `luanti_deprecated_register_on_auth_fail` | warn | `core.register_on_auth_fail`, use `core.register_on_authplayer` and check `is_success` |
| `luanti_deprecated_setting_get_pos` | warn | `core.setting_get_pos(name)`, use `core.settings:get_pos(name)` |
| `luanti_legacy_get_value_noise` | warn | `core.get_value_noise(seeddiff, octaves, persistence, spread)`, pass a noiseparams table |
| `luanti_deprecated_get_perlin` | warn | `core.get_perlin`, renamed to `core.get_value_noise` |
| `luanti_deprecated_get_mapgen_params` | warn | `core.get_mapgen_params()`, use `core.get_mapgen_setting(name)` |
| `luanti_deprecated_set_mapgen_params` | warn | `core.set_mapgen_params(params)`, use `core.set_mapgen_setting(name, value, override)` |
| `luanti_deprecated_get_node_group` | warn | `core.get_node_group(name, group)`, use `core.get_item_group(name, group)` |
| `luanti_deprecated_get_entity_name` | warn | `obj:get_entity_name()`, use `obj:get_luaentity().name` |
| `luanti_deprecated_get_player_velocity` | warn | `player:get_player_velocity()`, use `player:get_velocity()` |
| `luanti_deprecated_add_player_velocity` | warn | `player:add_player_velocity(vel)`, use `player:add_velocity(vel)` |
| `luanti_deprecated_get_look_pitch` | warn | `player:get_look_pitch()`, use `player:get_look_vertical()` |
| `luanti_deprecated_get_look_yaw` | warn | `player:get_look_yaw()`, use `player:get_look_horizontal()` |
| `luanti_deprecated_set_look_pitch` | warn | `player:set_look_pitch(radians)`, use `player:set_look_vertical(radians)` |
| `luanti_deprecated_set_look_yaw` | warn | `player:set_look_yaw(radians)`, use `player:set_look_horizontal(radians)` |

## Filter comments

change the severity of lints for part of a file with `allow`, `warn` or `deny`:

```lua
-- flint: allow(unused_variable)
local function unused() end

local a = 1 -- flint: allow(unused_variable, shadowing)

--# flint: allow(undefined_variable)
```

* a comment covers the statement it's in front of, on the same line as, or inside of. a comment before a function covers the whole function
* when filters overlap, the innermost one wins
* `--#` covers the whole file, and must come before any code
* selene's `-- selene: allow(...)` comments work the same way, and cover Luanti lints too
* filters for lints that don't exist and misplaced `--#` filters are reported as `invalid_lint_filter`
