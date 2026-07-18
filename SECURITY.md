# Security

## Supported versions

Cosmos Term is currently an early macOS release. Security fixes target the
latest tagged release and the `main` branch.

## Reporting a vulnerability

Please use GitHub's private vulnerability reporting for this repository.
Do not open a public issue for credentials exposure, command execution,
filesystem isolation, socket isolation, or protected-close bypasses.

Include:

- the affected Cosmos Term version
- a minimal reproduction
- expected and observed behavior
- whether the issue also reproduces in upstream WezTerm

Never include real passwords, tokens, private terminal output, or financial
data. Use disposable credentials and isolated tmux sockets in reproductions.

## Security model

Cosmos Term runs the commands you launch with your macOS user permissions. It
is not a sandbox. Shells and child applications may therefore trigger macOS
permission prompts under the Cosmos Term application identity.

Cosmos keeps its configuration, data, cache, runtime sockets, and protocol
environment separate from WezTerm. The optional protected-close integration
reads a local credential and never sends the entered passphrase to another
process.
