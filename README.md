# ASM — a personal fork of OpenAI Codex

ASM is Trung Ngo's personal fork of [OpenAI Codex](https://github.com/openai/codex), a coding agent that runs locally. The executable remains `codex`, and configuration and state remain in the shared Codex home.

## Install or update ASM

Published builds support macOS Apple Silicon and Linux x86_64 (MUSL).

```sh
curl -fsSL https://github.com/trungnt13/asm/releases/latest/download/install.sh | sh
codex
```

The installer downloads verified ASM release assets and installs `codex` with its `codex-code-mode-host` helper. Updates are manual: rerun the installer when you want to update. OpenAI installers, npm, and Homebrew packages install upstream Codex, not this fork.

## Reference

- [ASM releases](https://github.com/trungnt13/asm/releases)
- [Fork guide](docs/porting-from-codex.md): intended differences, development, validation, and releases.
- [Repository instructions](AGENTS.md): applicable coding and compatibility rules.
- [Upstream Codex documentation](https://developers.openai.com/codex): shared features; installation and update instructions there are for upstream Codex.

This repository is licensed under the [Apache-2.0 License](LICENSE).
