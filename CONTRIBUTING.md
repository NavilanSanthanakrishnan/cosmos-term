# Contributing to Cosmos Term

Thanks for helping improve Cosmos Term. This project is a focused WezTerm fork:
preserve terminal compatibility and place product behavior near explicit Cosmos
integration seams.

## Before opening a change

- Search existing issues and discussions.
- Report security-sensitive behavior privately through
  [SECURITY.md](SECURITY.md).
- Confirm whether a terminal-core issue also reproduces in upstream WezTerm.
- Keep pull requests focused and include user-visible intent.

## Development setup

Cosmos currently targets macOS on Apple silicon.

```sh
git clone --recurse-submodules \
  https://github.com/NavilanSanthanakrishnan/Cosmos-Terms.git
cd cosmos-term
cargo test -p cosmos-workspace
cargo check -p wezterm-gui -p wezterm -p wezterm-mux-server
```

For a release bundle:

```sh
cargo build --release -p wezterm-gui -p wezterm -p wezterm-mux-server
ci/package-cosmos-macos.sh
codesign --verify --deep --strict "dist/Cosmos Term.app"
plutil -lint "dist/Cosmos Term.app/Contents/Info.plist"
```

## Code map

| Area | Location |
| --- | --- |
| Explorer state, directory cache, workers | `cosmos-workspace` |
| Native Explorer rendering and input | `wezterm-gui/src/termwindow/cosmos.rs` |
| Bundled defaults | `assets/cosmos/cosmos.lua` |
| macOS packaging | `ci/package-cosmos-macos.sh` |
| Architecture decisions | `docs/cosmos-architecture.md` |
| Live and isolation tests | `docs/cosmos-testing.md` |

Changes to upstream modules should be narrow, documented, and justified when a
Cosmos seam cannot contain the behavior.

## Safety rules

- Never stop, reconfigure, or reuse a contributor's installed WezTerm process.
- Never mutate the default tmux server during tests.
- Every mutating tmux test must use an explicit dedicated socket:

  ```sh
  tmux -S /tmp/cosmos-term-test-$$.sock ...
  ```

- Keep Cosmos config, data, cache, sockets, bundle ID, and protocol environment
  distinct from WezTerm.
- Do not commit generated `dist/` bundles, logs, runtime state, raw sessions,
  credentials, personal terminal output, or absolute user paths.
- Preserve upstream attribution and bundled third-party licenses.

For an isolated Cosmos test process, set `COSMOS_TERM_DATA_DIR` and
`COSMOS_TERM_RUNTIME_DIR` to disposable paths.

## Tests

Every change must pass:

```sh
cargo test -p cosmos-workspace
cargo check -p wezterm-gui -p wezterm -p wezterm-mux-server
git diff --check
```

Renderer, app-icon, packaging, or release changes must also exercise the
relevant matrix in [docs/cosmos-testing.md](docs/cosmos-testing.md).

Add unit coverage to `cosmos-workspace` for reusable behavior. Keep directory
scans, pane resolution, filesystem events, Git, and local status reads off the
render thread.

## Pull requests

Explain:

- what changed and why
- the user impact
- the checks you ran
- any upstream WezTerm files touched
- screenshots for visible changes, with private terminal content removed

By contributing, you agree that your changes are distributed under the
repository's MIT license.
