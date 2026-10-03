---
title: Config
---

flint reads `flint.toml` from the root of whatever it is run on (`--path`, defaulting to the current directory). the layout loosely follows [biome](https://biomejs.dev/reference/configuration/). every section and key is optional.

see [example.flint.toml](https://github.com/luanti-org/flint/blob/master/example.flint.toml) for a full example with the defaults.

| section | purpose |
|---|---|
| `[linter]` | passed to [selene](https://kampfkarren.github.io/selene/usage/configuration.html). `std` and `exclude` aren't supported |
| `[formatter]` | passed to [stylua](https://github.com/JohnnyMorganz/StyLua#options). `syntax` isn't supported, LuaJIT is always used |
| `[vcs]` | `enabled`, `client_kind` (only `git`), and `use_ignore_file`. unlike biome, ignore files are respected by default |
| `[files]` | `includes`: globs of files to operate on, defaulting to `["**/*.lua"]`. a leading `!` excludes, and the last matching glob wins |

hidden files and directories are always skipped. `selene.toml`, `stylua.toml`, and `.styluaignore` aren't read.
