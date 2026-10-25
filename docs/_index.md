---
title: Flint (tool)
bookCollapseSection: true
---

> [!WARNING]
> This tool is in early dev/beta. use at your own risk and expect changes

flint is a CLI for working on mods, modpacks, games, and texture packs. run it from the package root or pass `--path`.

| command | purpose |
|---|---|
| [format](format.md) | format lua with stylua |
| [lint](lint.md) | lint lua with selene, plus luanti specific lints and fixes |
| [modernize](modernize.md) | apply every safe lint fix |
| [generate](generate.md) | create/update package files from source and conf files |
| [convert](convert.md) | convert models and translations between formats |
| [compress](compress.md) | losslessly shrink png and ogg files |

see [config](config.md) for `flint.toml` and [CI](ci.md) for running in GitHub Actions.
