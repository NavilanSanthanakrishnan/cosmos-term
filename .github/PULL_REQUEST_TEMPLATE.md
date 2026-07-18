## Summary

Describe the user-visible change and why it belongs in Cosmos Term.

## Validation

- [ ] `cargo test -p cosmos-workspace`
- [ ] `cargo check -p wezterm-gui -p wezterm -p wezterm-mux-server`
- [ ] `git diff --check`
- [ ] I used a dedicated `tmux -S /tmp/...` socket for every mutating tmux test.
- [ ] I did not stop or reconfigure an installed WezTerm or unrelated Cosmos Term process.

## Screenshots

Add before/after captures for visible changes, with private terminal content removed.

## Upstream impact

List the upstream WezTerm files touched and explain why a Cosmos integration
seam was not sufficient.
