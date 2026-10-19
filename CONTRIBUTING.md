# Contributing to Voltaris

Thanks for helping make Voltaris better.

## Report a problem

Open an [issue](https://github.com/NextBuilder/Voltaris/issues) with:

- what you asked Claude to do
- what happened, and what you expected
- your OS, KiCad version and Voltaris version
- the output of the `get_installation_info` tool, if the server starts

## Suggest a change

1. Fork the repo and create a branch.
2. Build and test:
   ```bash
   cargo build -p voltaris
   cargo test --workspace
   cargo fmt --all -- --check
   ```
3. If you changed skills or agents, regenerate the Claude plugin:
   `python packaging/build-claude-plugin.py`
4. Open a pull request that explains what changed and how you tested it.

The developer guide is in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md), and the full tool list
is in [docs/TOOLS.md](docs/TOOLS.md).

By contributing you agree that your work is licensed under the
[AGPL-3.0](LICENSE), like the rest of Voltaris.
