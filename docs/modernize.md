---
title: Modernize
---

converts old file formats to new ones. the package type is detected from its files:

| type | detected by | conf file |
|---|---|---|
| game | `game.conf` | `game.conf` |
| modpack | `modpack.conf` or `modpack.txt` | `modpack.conf` |
| mod | `mod.conf` or `init.lua` | `mod.conf` |
| texture pack | `texture_pack.conf` | `texture_pack.conf` |

for all types:

* `description.txt` is moved into the `description` of the conf file
* `locale/*.tr` translations are converted to `locale/*.po`

additionally:

* mods: `depends.txt` is moved into `depends` and `optional_depends` of `mod.conf`. if a mod has no `mod.conf`, one is created with `name` set to the folder name
* modpacks: `modpack.txt` is replaced by `modpack.conf`

if an old file doesn't match what's already in the conf file, nothing is changed and an error is reported.