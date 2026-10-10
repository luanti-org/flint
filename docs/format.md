---
title: Format
---

calls [stylua](https://github.com/JohnnyMorganz/StyLua)

configured by `[formatter]` in [flint.toml](config.md)

```sh
flint format
flint format --check
```

| option | purpose |
|---|---|
| `--check` | don't write anything, list the files that aren't formatted and fail if there are any. useful in [CI](ci.md) |
