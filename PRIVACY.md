# Privacy

Cosmos Term does not include telemetry, analytics, advertising, or an automatic
update request. No Cosmos usage data is sent to the project maintainers.

## Local data

Cosmos stores application state below its own macOS data and cache locations:

- `~/Library/Application Support/cosmos-term`
- `~/Library/Caches/cosmos-term`

This includes Explorer layout/expansion state, runtime sockets, logs, and
normal terminal convenience state inherited from WezTerm. Terminal scrollback
is maintained by the running terminal process.

The optional status bar reads only structured Codex `token_count` events from
the local Codex session directory and enumerates local process executable names
to count processes named exactly `codex`. It does not read prompt/response text,
invoke the Codex CLI, or send that information over the network.

Git decorations run local `git status` commands for the folder displayed in
the Explorer. tmux pane following may run a local `tmux display-message`
command against the tmux server to which the Cosmos process was intentionally
attached.

## Child-process permissions

Cosmos Term launches shells and applications with your macOS user permissions;
it is not a sandbox. macOS may attribute permission requests from a command you
run—such as camera, Contacts, Documents, or Downloads access—to Cosmos Term
because Cosmos is the parent application.

## Optional protected close

When a local close-lock credential exists, Cosmos verifies the entered
passphrase inside the application. The passphrase is not logged or placed in a
child-process argument. After successful verification, a configured local
autosave command may run before close.

## Network behavior

Cosmos inherits WezTerm's networking capabilities for commands and features
that users explicitly configure, such as SSH or remote multiplexer domains.
The Cosmos-specific Explorer and status bar do not create network requests.
The upstream automatic updater is disabled because Cosmos releases are not
WezTerm releases.

## Third-party builds

Anyone can modify open-source software. A binary obtained from a third party
may behave differently from this repository. Prefer release artifacts
published by this GitHub repository or build from source.
