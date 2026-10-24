---
title: Generate
---

creates/updates a package's files from it's  source code and conf files. existing files are updated, keeping what already exists.

```sh
flint generate
flint generate mod.conf settingtypes.txt
flint generate --check
```

with no args, every applicable file is created. trying to create an unsupported file for the content type errors.

| option | purpose |
|---|---|
| `--check` | don't write anything, list the files that are out of date and fail if there are any. useful in [CI](ci.md) |

| file | applies to | what's generated |
|---|---|---|
| `game.conf`, `modpack.conf`, `mod.conf`, `texture_pack.conf` | that type of package | see [conf files](#conf-files) |
| `settingtypes.txt` | mods | a line for each of the mod's own settings its code reads that isn't listed yet, see [settingtypes.txt](#settingtypestxt) |
| `pot` | mods and texture packs | `locale/<name>.pot`, and missing strings added to `locale/*.po`, see [translations](#translations) |
| `.cdb.json` | the package | [ContentDB](https://content.luanti.org/help/package_config/#cdbjson) metadata: `type`, and `title`, `short_description` from the conf, and `repo` and `issue_tracker` from the git `origin` remote. only keys that aren't set are added |
| `.luacheckrc` | the package, unless it's a texture pack | a [luacheck](https://github.com/lunarmodules/luacheck) config allowing the Luanti globals, the package's mods, the mods they depend on, and `globals` from [flint.toml](config.md). only created, an existing one is left alone |
| `.gitattributes` | the package | `* text=auto eol=lf`, and `export-ignore` for `.*` and `flint.toml` to leave them out of [ContentDB releases](https://content.luanti.org/help/package_config/#excluding-files). missing rules are added to an existing one |

## Conf files

* `name`: the folder name, if not set. mods and modpacks need one that's only `a-z`, `0-9` and `_`, otherwise set it yourself
* `title`: from the name if not set, e.g. `My Mod` for `my_mod`. for games, the deprecated `name` if set
* `optional_depends`: mods code checking for a mod with `core.get_modpath("name")` adds it, unless it's already a dependency. not done while a mod has a `depends.txt`, run `flint modernize` first
* `min_minetest_version`: set/raised based on the newest core function called
* `description`: never added, as flint can't guess what a package does. if it's missing, a note asks you to add one, without failing

## settingtypes.txt

follows the convention of [`luanti_missing_settingtype`](lint.md#luanti-lints): settings named `modname.*` or `modname_*`, read with a literal name, that aren't in the `settingtypes.txt` of the mod or of any modpack or game it's in. lines are added to the mod's own:

```lua
local speed = tonumber(core.settings:get("mymod.max_speed")) or 1.5
local enabled = core.settings:get_bool("mymod.enabled", true)
```

```
mymod.max_speed (Max speed) float 1.5
mymod.enabled (Enabled) bool true
```

the type comes from how it's read, and the default from a literal it falls back to: `get_bool(name, default)`, `get(name) or default`, `tonumber(get(name)) or default`, or `get_bool(name) ~= false` for `true`. without one, it's `false`, `0`, or empty. settings read with `get_flags` are never added, as flint can't guess their possible flags. a note asks you to add each by hand, without failing.

## Translations

`locale/<name>.pot` is a gettext template of the translatable strings, matching `xgettext` as recommended in [lua_api.md](https://github.com/luanti-org/luanti/blob/master/doc/lua_api.md#translations):

* the first argument of `S`, `NS`, `FS` and `NFS`, and the first two of `PS` and `FPS`. names assigned by `local S, PS = core.get_translator(domain)` are used however they're named
* `core.translate(domain, str)` and `core.translate_n(domain, str, str_plural, n)`
* only string literals, or literals joined with `..`
* a comment starting with `TRANSLATORS:` on the lines just before a string is included for translators, with the comment lines after it

```lua
-- TRANSLATORS: shown when a player joins
core.chat_send_player(name, S("Hello @1!", name))
```

the `title` and `description` of the mod or texture pack are included too, as Luanti translates them. so are those of a game or modpack, in the mod named after its `textdomain`, as Luanti looks for it in all its mods.

strings in a textdomain other than the mod's get it as their `msgctxt`. a template is made whenever there's something to translate, even if that's only the title or description.

each existing `locale/<domain>.<lang>.po` gets the strings of its textdomain that it's missing, untranslated, with as many plural forms as its `Plural-Forms`, or the language's if it has none. nothing already in it is changed.
