---
title: Lint
---

calls [selene](https://github.com/Kampfkarren/selene)

* supports luanti globals by default
* allows calling a globals that are the name of mods dep'd on
* allows extra globals listed in `globals` under `[linter]`, e.g. `globals = ["mylib"]`
* configured by `[linter]` in [flint.toml](config.md)

## Fixes

some lints can be fixed automatically, handled by `flint lint --write`. run `flint lint --write` (or `flint modernize`) to apply the fixes, then report what's left. lints set to `allow`, in the config or by a [filter comment](#filter-comments), aren't fixed.

some deprecated functions are replaced by something added in a newer engine. the mod's `min_minetest_version` decides what happens, which is the highest `min_minetest_version` in the conf of the mod and of any modpack or game it's in:

* at least the version the replacement was added in: the lint is reported and fixable
* lower: the lint isn't reported, as the mod supports engines without the replacement
* not set: the mod might support every version, like ContentDB assumes, so the lint is reported with a note to set `min_minetest_version`, but not fixed

## Luanti lints

flint adds its own lints on top of selene's. they're configured the same way, in `[linter.lints]` and `[linter.config]`.

| lint | default | description |
| --- | --- | --- |
| `luanti_legacy_physics_override` | deny | `player:set_physics_override(speed, jump, gravity)` instead of a table, e.g. `player:set_physics_override({ speed = 1.5 })`. fixable, unless the last argument is a call or `...` |
| `luanti_deprecated_env` | deny | `core.env` or `minetest.env`, on its own or with anything after it. its methods are functions on `core`, e.g. `core.get_node(pos)` instead of `core.env:get_node(pos)`. fixable when something follows `env` |
| `luanti_minetest_global` | warn | the `minetest` global instead of `core`, e.g. `core.log("hi")` instead of `minetest.log("hi")`. fixable, unless the file has a local named `core` |
| `luanti_missing_settingtype` | warn | a mod reads its own setting, e.g. `core.settings:get("mymod.speed")`, that isn't in `settingtypes.txt`. only settings named `modname.*` or `modname_*` with a literal name are checked, since others may belong to the engine or other mods. the `settingtypes.txt` of the mod and any modpack or game it's in count |
| `luanti_deprecated_register_on_auth_fail` | warn | `core.register_on_auth_fail`, use `core.register_on_authplayer` and check `is_success` |
| `luanti_deprecated_setting_get_pos` | warn | `core.setting_get_pos(name)`, use `core.settings:get_pos(name)`, added in 5.11. fixable where it's called, when the mod's `min_minetest_version` is at least `5.11` |
| `luanti_legacy_get_value_noise` | warn | `core.get_value_noise(seeddiff, octaves, persistence, spread)`, pass a noiseparams table |
| `luanti_deprecated_get_perlin` | warn | `core.get_perlin`, renamed to `core.get_value_noise` in 5.12. fixable when the mod's `min_minetest_version` is at least `5.12` |
| `luanti_deprecated_get_mapgen_params` | warn | `core.get_mapgen_params()`, use `core.get_mapgen_setting(name)` |
| `luanti_deprecated_set_mapgen_params` | warn | `core.set_mapgen_params(params)`, use `core.set_mapgen_setting(name, value, override)` |
| `luanti_deprecated_get_node_group` | warn | `core.get_node_group(name, group)`, use `core.get_item_group(name, group)`. fixable |
| `luanti_deprecated_get_entity_name` | warn | `obj:get_entity_name()`, use `obj:get_luaentity().name` |
| `luanti_deprecated_get_player_velocity` | warn | `player:get_player_velocity()`, use `player:get_velocity()`, which works on players since 5.4. fixable when the mod's `min_minetest_version` is at least `5.4` |
| `luanti_deprecated_add_player_velocity` | warn | `player:add_player_velocity(vel)`, use `player:add_velocity(vel)`, which works on players since 5.4. fixable when the mod's `min_minetest_version` is at least `5.4` |
| `luanti_deprecated_get_look_pitch` | warn | `player:get_look_pitch()`, use `player:get_look_vertical()` |
| `luanti_deprecated_get_look_yaw` | warn | `player:get_look_yaw()`, use `player:get_look_horizontal()` |
| `luanti_deprecated_set_look_pitch` | warn | `player:set_look_pitch(radians)`, use `player:set_look_vertical(radians)`. fixable |
| `luanti_deprecated_set_look_yaw` | warn | `player:set_look_yaw(radians)`, use `player:set_look_horizontal(radians)`. fixable |

## Package lints

these check a package's files rather than its Lua, for the package being linted and the mods and modpacks inside it. the package type is detected from its files:

| type | detected by | conf file |
|---|---|---|
| game | `game.conf` | `game.conf` |
| modpack | `modpack.conf` or `modpack.txt` | `modpack.conf` |
| mod | `mod.conf` or `init.lua` | `mod.conf` |
| texture pack | `texture_pack.conf` | `texture_pack.conf` |

| lint | default | description |
| --- | --- | --- |
| `luanti_missing_mod_conf` | warn | a mod without a `mod.conf`. the fix creates one with `name` set to the folder name |
| `luanti_legacy_modpack_txt` | warn | `modpack.txt`. the fix replaces it with `modpack.conf` |
| `luanti_legacy_description_txt` | warn | `description.txt`. the fix moves it into `description` of the conf file |
| `luanti_legacy_depends_txt` | warn | a mod's `depends.txt`. the fix moves it into `depends` and `optional_depends` of `mod.conf` |
| `luanti_legacy_tr_translation` | warn | `locale/*.tr` translations. the fix converts them to gettext `locale/*.po` |

if an old file doesn't match what's already in the conf file or `.po`, it can't be fixed and nothing is changed. these can't be changed by filter comments, only in `[linter.lints]`.

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
