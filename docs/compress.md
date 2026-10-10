---
title: Compress
---

losslessly compresses the media in a mod, modpack, game, or texture pack, shrinking files in place:

| files | tool | what changes |
|---|---|---|
| `*.png` | [oxipng](https://github.com/oxipng/oxipng) | re-encodes the image data and strips metadata chunks that don't affect how it's displayed (text, timestamps, etc.). pixels are unchanged |
| `*.ogg` | [OptiVorbis](https://github.com/OptiVorbis/OptiVorbis) | re-packs the Vorbis stream. the decoded audio is unchanged |

extensions are matched regardless of case. a file is only modified when the result is smaller

```sh
flint compress
flint compress --level 6 --path path/to/mod
```

| option | default | purpose |
|---|---|---|
| `--level` | `2` | oxipng optimization level from `0` to `6`. higher is smaller but slower. doesn't affect `.ogg` files |

files are found the same way as the other commands (see [config](config.md)): only those matching `[files]` `includes` are compressed, hidden files and directories are skipped, and `.gitignore` is respected unless disabled in `[vcs]`. e.g. `includes = ["**", "!textures/vendor/**"]` leaves vendored textures alone.

files that have issues (i.e. corrupted) are left untouched and reported back.
