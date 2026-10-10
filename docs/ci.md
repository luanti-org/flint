---
title: CI
---

flint can run in GitHub Actions as an action:

```yaml
name: flint

on: [push, pull_request]

jobs:
  flint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: luanti-org/flint@master
        with:
          command: lint
      - uses: luanti-org/flint@master
        with:
          command: format
      - uses: luanti-org/flint@master
        with:
          command: compress
```

| input | default | purpose |
|---|---|---|
| `command` | `lint` | flint command to run |
| `path` | `.` | directory to run on, relative to the repository root |
| `check` | `true` | pass `--check` to `format` and `compress`, so they fail instead of writing changes |

flint is built from source the first time and cached afterwards, so the first run takes a few minutes.

when run in GitHub Actions, issues are also reported as [annotations](https://docs.github.com/en/actions/reference/workflow-commands-for-github-actions#setting-an-error-message), so they show up inline on pull requests. this happens whenever `GITHUB_ACTIONS=true`, including when running flint yourself in a workflow without the action.
