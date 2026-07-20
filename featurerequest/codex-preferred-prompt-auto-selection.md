# Codex Preferred Prompt Auto-Selection

Status: Implemented
Scope: Native and tmux Codex panes

## Summary

Add a native Cosmos Term automation that recognizes a small, explicit set of
Codex model-choice prompts and selects the user's preferred non-switching
response while the computer is unattended.

This must be prompt-aware and fail closed. It must not behave like a generic
timer that periodically presses Enter, use screenshot OCR, or act on
approvals, permissions, destructive confirmations, or arbitrary menus.

## Prompt policy

| Codex prompt | Preferred response |
| --- | --- |
| **Additional safety checks** — offers a faster-model retry, waiting, and learn-more choices | **Keep waiting** |
| **Approaching rate limits** — offers a lower-cost model or the current model | **Keep current model** |
| **Codex just got an upgrade** — offers a new model or the existing model | **Use existing model** |

For the rate-limit prompt, the exact target is **Keep current model**, not
**Keep current model (never show again)**. The feature should make the choice
when the prompt appears without silently changing Codex's future reminder
settings.

OpenAI's service-side safety checks and routing are outside this feature. A
terminal keypress can control only a local Codex selection prompt; it cannot
prevent server-side blocking, buffering, or rerouting.

## Goals

- Preserve the model already selected for the Codex session.
- Keep safety-buffered work waiting instead of retrying on a smaller model.
- Work without bringing Cosmos Term to the foreground.
- Cover native Cosmos panes and Codex processes inside tmux.
- Handle each recognized prompt exactly once.
- Leave every unrecognized prompt untouched.
- Add no daemon, Accessibility permission, mouse automation, or screen OCR.

## Non-goals

- Automatically approve commands, file changes, permissions, or MCP actions.
- Answer arbitrary numbered menus.
- Accept upgrades, install updates, buy credits, notify account owners, or
  request limit increases.
- Disable OpenAI safeguards or alter service-side routing.
- Replace Codex configuration when an official durable setting already
  satisfies the preference.

## Implemented architecture

### 1. Strict prompt registry

The pure Rust prompt classifier lives in `cosmos-workspace`. Each rule requires a
complete prompt signature: a stable title or heading plus the expected option
labels in one bounded terminal region.

The registry returns only one of three actions:

- `KeepWaiting`
- `KeepCurrentModel`
- `UseExistingModel`

Matching a single phrase is insufficient. For example, ordinary conversation
text containing “keep waiting” must not activate the feature.

The initial registry should be derived from the installed Codex 0.144.6 source
paths:

- `codex-rs/tui/src/chatwidget/safety_buffering.rs`
- `codex-rs/tui/src/chatwidget/rate_limits.rs`
- `codex-rs/tui/src/model_migration.rs`

Prompt fixtures should be versioned so a Codex update can be reviewed before
new or changed wording becomes actionable.

### 2. Terminal-cell detection

Use Cosmos Term's existing mux pane-output notifications. After a short
150–250 ms stabilization delay, read only the bounded visible terminal cells
for the changed pane and normalize styling without losing row boundaries.

Detection should be output-driven for native panes rather than a permanent
high-frequency poll. It must not run filesystem work or prompt parsing from the
render path.

### 3. Numeric shortcut selection

Codex's relevant selection views display numeric choices and accept the
corresponding number as an immediate selection. After finding the exact target
label, extract its displayed number and inject that single literal digit.

This is safer than sending Down followed by Enter:

- **Keep waiting** can be option 1 or option 2.
- The target is selected by its rendered label rather than assumed position.
- A single input event reduces redraw races.

If the target has no unambiguous displayed shortcut, do nothing.

### 4. Native Cosmos panes

For a native pane:

1. Confirm that the foreground executable basename is exactly `codex`.
2. Confirm that no Cosmos overlay or file-workspace input mode owns the pane.
3. Read the current terminal screen through the existing `Pane` interface.
4. Run the strict classifier.
5. Send the literal numeric key through the pane's existing key-input path.

Inactive tabs owned by the same Cosmos window should remain eligible when
their mux pane emits output, without changing the selected tab or stealing
focus.

### 5. Tmux panes

Extend the existing pane-context worker with the minimum metadata needed to
identify exact inner Codex panes, including current command, pane mode, window,
geometry, and server identity.

For tmux:

1. Use bounded `tmux capture-pane` output for candidate panes whose current
   command is exactly `codex`.
2. Run the same strict classifier off the render thread.
3. Send one literal numeric key to the exact pane with targeted
   `tmux send-keys`.
4. Do not select the pane, switch windows, or change the attached client's
   layout.

The implementation should cover visible, inactive, and detached Codex panes
associated with a Cosmos-managed tmux server. Multiple Cosmos views of the
same server must share a server-and-pane deduplication key so they cannot act
twice.

### 6. State machine and deduplication

Track prompt state by pane, prompt kind, option fingerprint, and terminal
generation.

The window integration tracks these states:

1. `Candidate`
2. `Stable`
3. `ActionSent`
4. `Cleared`
5. `FailedClosed`

After sending a choice, wait for the recognized prompt to disappear. If it
does not clear, record a failure and do not send another key. Keep the
fingerprint suppressed until the complete prompt disappears so timer redraws
cannot cause repeated digits to reach the normal composer.

Pause automation briefly after manual user input in that pane to avoid racing
with an actively handled prompt.

## Safety requirements

An action is permitted only when all of these conditions are true:

- The target process is exactly Codex.
- A complete whitelisted prompt signature is present.
- Exactly one preferred option is present.
- Its numeric shortcut is unambiguous.
- The prompt is stable across the debounce window.
- The pane is not in copy mode or owned by another Cosmos input surface.
- The prompt fingerprint has not already been handled.
- No recent manual input indicates that the user is interacting with it.

The detector must explicitly reject:

- command or file-change approvals;
- sandbox and permission prompts;
- update prompts;
- add-credit or notify-owner prompts;
- close and quit confirmations;
- request-user-input questions;
- MCP elicitation;
- ordinary terminal content containing similar language.

## Controls and observability

Expose three modes:

- `off`
- `observe`
- `active`

`observe` should perform full detection and record what would have happened
without sending input. Rollout must begin in this mode.

Record only minimal local metadata:

- timestamp;
- prompt kind;
- opaque server/pane identifier;
- chosen policy action;
- result (`observed`, `sent`, `cleared`, or `failed_closed`).

Do not log terminal contents, prompts, responses, commands, paths, or
conversation data.

Provide an immediate runtime off switch. A compact footer indicator may show
that automation is enabled and the count of choices made, but it must not
produce a toast for every successful selection.

## Relationship to current Codex configuration

The current local Codex customization suppresses the safety-buffering chooser,
and the GLX profile suppresses the optional rate-limit model nudge. Keep those
protections during development.

The Cosmos feature should still recognize these prompts as a fallback for:

- non-GLX sessions;
- unpatched Codex binaries;
- a Codex update that restores upstream prompt behavior; and
- a future decision to leave prompts enabled but auto-select the preferred
  response.

Only after `observe` and isolated active tests pass should the existing
suppressions be reconsidered. Their removal is not required for this feature
to provide defense in depth.

## Validation plan

### Unit tests

- All three prompt families at wide and narrow terminal widths.
- Safety buffering both with and without a faster-model option.
- Exact distinction between **Keep current model** and **Keep current model
  (never show again)**.
- Wrapped option descriptions.
- Duplicate or missing target options.
- Old matching text remaining in scrollback.
- Ordinary user or model text containing prompt phrases.
- Approval, permission, update, and request-user-input false positives.
- Deduplication across redraws and timer updates.

### Native pane integration

Use a disposable synthetic terminal pane that renders captured Codex fixtures.
Verify:

- one expected numeric key is sent;
- the prompt clears;
- no Enter, arrow, mouse, or unrelated key is sent;
- inactive tabs do not become active;
- unknown prompts receive no input;
- one failed verification never retries into the composer.

### Isolated tmux integration

Use a unique socket for every test:

```sh
tmux -S /tmp/cosmos-codex-prompts-<unique>.sock
```

Cover:

- active and inactive splits;
- another tmux window;
- a detached session;
- two Cosmos views of one server;
- a non-Codex process displaying an identical fixture;
- copy mode and command-prompt mode;
- prompt redraws after the action.

Assert that only the exact target pane receives one digit. Record the user's
default tmux clients before and after and verify that they remain unchanged.

### Performance and isolation

- Reuse existing Cosmos workers and output notifications where possible.
- Keep terminal parsing bounded to visible rows.
- Avoid helper daemons and broad process scans.
- Measure idle CPU and physical footprint against the documented Cosmos
  baseline.
- Run the first packaged build with isolated Cosmos data/runtime roots.
- Confirm installed WezTerm, live Cosmos, and the default tmux server are
  unchanged throughout disposable testing.

## Controls

- The persisted default is `observe`.
- `Command+Option+P` cycles `off` → `observe` → `active` → `off`.
- `Command+Option+Escape` changes immediately to `off`, clears pending
  candidates, and closes the worker action gate.
- The status footer shows `Prompts off`, `Prompts observe`, or
  `Prompts active`, followed by the successful-choice count when nonzero.

The worker's action gate defaults to disabled and is checked again immediately
before a targeted tmux send. Changing from active to off therefore fails closed
even if a scan completed concurrently.

## Implemented rollout

1. The versioned classifier and false-positive fixtures are implemented.
2. Native-pane detection is output-driven with a 200 ms stabilization delay.
3. Tmux scans and revalidated targeted sends run on the existing context
   worker.
4. Product default and state migration use `observe`.
5. A process-wide opaque target/fingerprint reservation prevents two Cosmos
   windows from acting on the same prompt.
6. The exact target remains suppressed until the complete prompt disappears.
7. An owner-only JSONL audit stores metadata only.
8. The immediate off switch and worker action gate remain available in every
   window.

## Definition of done

- Every recognized prompt is classified once; active mode sends the preferred
  response once.
- The selected Codex model remains unchanged.
- No recognized safety-buffering request is retried on the faster model.
- Native, inactive-tab, and targeted tmux-pane operation works without focus
  changes.
- Detached tmux Codex panes are covered without attaching or rearranging them.
- No approval, permission, destructive, or unknown prompt receives input.
- A failed or ambiguous match sends nothing.
- The feature adds no meaningful idle CPU or memory regression.
- The user's installed WezTerm and default tmux state remain untouched during
  testing.

## Verification completed

- Unit fixtures cover all three prompt families, the two safety-check option
  shapes, the exact current-model/reminder distinction, incomplete and
  ambiguous menus, approvals, and stale scrollback.
- A Rust integration test creates a dedicated
  `/tmp/cosmos-codex-prompts-<unique>.sock` server and an executable named
  exactly `codex`, renders the full rate-limit prompt, revalidates it, sends
  only digit `2` to the exact pane, and removes the server.
- `cargo test -p cosmos-workspace` passes 28 tests.
- `cargo check -p wezterm-gui -p wezterm -p wezterm-mux-server`,
  release packaging, strict signature validation, plist validation, and
  `git diff --check` pass.
- The user's default tmux client remained unchanged throughout the isolated
  integration test.
