# stay v2: implementation plan (tmux-backed rewrite)

Date: 2026-07-20. This is a cleanroom rewrite of `stay` v1 (currently
`~/stay/stay.old`, referred to below as **v1**). v1 implements its own
client/server daemon, PTY handling, and a full ANSI/grid terminal emulator (~28k
lines across `ansi/`, `grid/`, `scrollback/`, `protocol.rs`, `server.rs`,
`socket.rs`, `socket_lifecycle.rs`, `security.rs`, `client.rs`, `child.rs`,
`history.rs`). That engine exists to solve problems tmux already solved years
ago and hardened in production. **v2 deletes all of it** and becomes a thin Rust
CLI that wraps the `tmux` binary, keeping only what tmux doesn't provide: a
cleaner CLI, an interactive picker, single-key detach/copy-mode UX, and
terminated-session post-mortem review.

**This plan must be implementable without opening v1's source at all.**
`~/stay/stay.old/src` has been deliberately deleted — not merely set aside —
specifically so that whoever implements v2 cannot see, reference, or
unconsciously copy v1's actual code/architecture. This is a cleanroom rewrite in
the literal sense: the new implementation should be derived from this plan and
from v1's externally-observable *behavior*, not from reading or adapting v1's
Rust. `~/stay/stay.old/dev_docs`, `docs`, `tests`, `README.md`, and similar are
kept precisely because they describe that observable behavior (user-facing docs,
design rationale, and — especially — the test suite's asserted behavior) without
exposing the implementation that produced it; these are fair, useful reference
material for cross-checking this plan against what v1 actually guarantees. **The
source is also not recoverable via `git log`/`git show` inside `stay.old`'s
history** — digging it back out of version control to peek at defeats the same
purpose as reading it directly off disk, and should not be done. Every
behavioral claim this plan makes about v1 (a CLI flag's effect, a test name, a
signal-handling detail, etc.) is stated inline, in enough detail to implement
against, precisely so no implementer ever needs to go looking for the source to
fill in a gap. If a genuine gap is found where the plan references v1 behavior
without enough detail to implement it, that's a bug in this plan to fix by
expanding the plan itself (from the surviving docs/tests) — not a cue to go read
the deleted source.

## Decisions locked in with the user

| Question                            | Decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| ----------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Panes/multiplexing (windows/splits) | Not exposed. User runs i3wm and wants one shell per `stay` session, same philosophy as v1.                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Architecture depth                  | Thin wrapper. No stay daemon — the tmux server *is* the daemon.                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| Picker UI                           | Reimplemented as a native Rust TUI (not tmux's own `choose-tree`), same interaction model as v1 (arrows, Enter, c/v/e/l/r/k/Esc). Visually a fresh design, not a v1 mimic: screen takeover that probes the terminal at runtime and uses the alternate screen only where actually supported, falling back to an in-place main-screen redraw on terminals that ignore `?1049` (unlike v1's actual inline in-scrollback redraw), styled per ratatui best practice rather than v1's plain blue-highlight/`>`-prefix look. |
| Terminated-session post-mortem      | Keep it — built on tmux's `remain-on-exit`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| Detach key                          | Keep single-key detach (no tmux prefix step) via a thin client-side interceptor.                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| Scrollback / copy-mode              | Not tmux's default prefix binding — a single configurable key enters tmux copy-mode directly. Exit needs no interception: tmux's default (emacs) key table already binds `Escape` → `cancel` (exit copy-mode) natively, confirmed in `man tmux`'s copy-mode key bindings.                                                                                                                                                                                                                                             |
| Attach-mode flags (-r/-l/-p)        | Map onto tmux's own attach-session flags/mechanisms rather than reimplementing v1's exact semantics.                                                                                                                                                                                                                                                                                                                                                                                                                  |
| CLI compatibility with v1           | Free to evolve — not required to match byte-for-byte.                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Security model                      | Trust tmux's own socket security (0700, per-user `$TMPDIR/tmux-$UID`). No peer-UID/TOCTOU layer of our own.                                                                                                                                                                                                                                                                                                                                                                                                           |
| tmux dependency                     | Required external dependency, recent version only (target tmux ≥ 3.2; dev/test machine has 3.6a).                                                                                                                                                                                                                                                                                                                                                                                                                     |
| Low-priority mapping                | `-l` attaches with tmux's `ignore-size` client flag (doesn't become the client that drives session size). Normal attach uses tmux's default sizing behavior.                                                                                                                                                                                                                                                                                                                                                          |
| Multi-host                          | Out of scope — local machine only.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Config                              | Keep TOML + env var overrides, same mechanism as v1, trimmed/renamed key set.                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| Project location                    | `~/stay/stay` is the new project going forward.                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |

## Why this is possible: what tmux already gives us for free

| v1 component (custom-built)                                    | tmux equivalent (already exists)                                                                           |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `socket.rs` fork/daemonize orchestration                       | tmux server auto-starts on first `new-session`, persists independently of any client                       |
| `socket_lifecycle.rs` stale-socket TOCTOU quarantine           | tmux owns its own socket lifecycle; no stale-socket problem to guard against                               |
| `security.rs` peer-UID verification, elevated-privilege checks | tmux socket dir is already `0700`, per-UID, mature and audited                                             |
| `pty.rs` + `child.rs` PTY allocation, child process, SIGWINCH  | tmux does this per-pane already                                                                            |
| `ansi/`, `grid/` full ANSI/VT parser + cell grid               | tmux's own client does all terminal emulation; we never touch escape sequences                             |
| `scrollback/`, `history.rs` retained-output buffer, replay     | tmux's own scrollback (`history-limit`) + `capture-pane`                                                   |
| `protocol.rs` bespoke binary wire protocol                     | tmux's own client/server protocol (we never see it — we shell out to the `tmux` CLI)                       |
| `client_set.rs` multi-client min-geometry resize policy        | tmux's own `window-size` option (`largest`/`smallest`/`manual`/`latest`) and per-client `ignore-size` flag |

What's left for stay to build is genuinely small: CLI/config, a picker, and a
thin key-interception relay for the two single-key UX features (detach,
copy-mode entry/exit) that tmux doesn't offer without its prefix key.

## Architecture

```
stay (Rust binary)
  ├── cli.rs        — clap parsing, flag → behavior dispatch
  ├── config.rs      — TOML + env var loading (trimmed from v1)
  ├── tmux.rs        — thin wrapper: builds/runs `tmux -L stay <subcommand>` argv,
  │                     parses list-sessions/list-panes output
  ├── session.rs      — create/attach/kill/force-recreate orchestration over tmux.rs
  ├── relay.rs        — the thin PTY relay + 2-key interception (replaces v1's
  │                     client.rs; no grid/ANSI, just byte passthrough plus a
  │                     one-shot side command on the detach/copy-mode-entry bytes)
  ├── logging.rs       — pipe-pane / capture-pane wiring for -L/-t/-s
  ├── picker/          — ratatui interactive session list
  └── main.rs
```

**Session namespace.** All stay-managed sessions live under a dedicated tmux
server socket, `tmux -L stay ...` (tmux's `-L` picks a named socket resolved
under tmux's own default per-user runtime directory, which tmux already
secures). This keeps stay's session list scoped to sessions *it* created — it
never lists or touches sessions the user starts with plain `tmux` for other
purposes, or vice versa. No directory-path override is offered (unlike v1's
`STAY_SOCKET_DIR`) — the user confirmed this isn't needed; tmux's own default
location is sufficient, and not exposing it removes a knob that would need its
own security reasoning (v1's version existed to let the *directory* be placed
under stay's own validated-and-secured path, which mattered when stay owned the
security model — moot now that tmux owns its own socket security).

**No stay daemon.** `stay <name>` either shells out to `tmux new-session -d`
(detached) or runs the relay in the foreground; there is nothing of stay's own
running in the background. Killing all sessions under the `stay` socket lets
tmux's own server exit naturally — stay does not need to manage that.

**Relay signal and terminal safety.** `relay.rs` is directly analogous to v1's
`client.rs` — a foreground process holding the real terminal in raw mode — so
the same hard-won correctness lessons apply, and were missing from the first
draft of this plan:

- **SIGTERM → clean detach.** An external SIGTERM (logout, process-manager
  shutdown, `kill`) must cause the relay to detach gracefully (run the same
  `detach-client` side call the detach key triggers, restore the terminal, exit
  0\) rather than dying uncleanly mid-relay and leaving the outer terminal in raw
  mode.
- **SIGPIPE ignored.** Writing to the `tmux attach-session` child's stdin after
  it's already exited (e.g. the session was killed from elsewhere mid-attach)
  must not deliver a default-disposition SIGPIPE that kills the relay outright;
  ignore it and let the normal "child exited" path handle cleanup.
- **Panic-safe terminal restoration.** If the release build uses
  `panic = "abort"` (as v1's does), `Drop`-based termios restoration never runs
  on a panic. Install a `std::panic::set_hook` that restores cooked mode before
  the abort, mirroring v1's fix (`terminal.rs`) — otherwise a panic inside the
  relay leaves the user's shell in raw mode after stay exits.
- **Real PTY regardless of stay's own stdio.** The relay must allocate and hand
  `tmux attach-session` a genuine PTY unconditionally — even when stay's own
  stdin/stdout are redirected, piped, or `/dev/null` (e.g.
  `stay build cargo build > log.txt`, or stay's own test harness running fully
  non-interactively). `tmux attach-session` is known to refuse or misbehave
  without a real controlling terminal, and this non-TTY case is a heavily-used
  v1 pattern (its own test suite runs this way) that must keep working.
- **`setsid` + `TIOCSCTTY` on the relay's own child.** The relay's spawn of
  `tmux attach-session` needs the same session-leader/controlling-terminal setup
  v1's `pty.rs` gives its own child, so that abruptly killing the relay
  (SIGKILL/SIGQUIT) closes the PTY master and the kernel cleanly HUPs the
  orphaned `tmux attach-session` — detaching that client — rather than leaving a
  dangling attached client the session doesn't know is gone.
- **DEC private-mode restore bracket — needs empirical verification, not
  assumed.** v1's `terminal.rs` saves/restores ~19 DEC private modes (mouse
  tracking, bracketed paste, synchronized-output, etc.) around every attach,
  because leaving one of these set after detach corrupts the *outer* shell's
  terminal behavior. Before deciding whether `relay.rs` needs to replicate this:
  verify whether tmux's own attach-session client already resets these modes
  cleanly on detach (plausible, since tmux is the one turning them on in the
  first place for its own copy-mode/mouse features) — if it doesn't, port v1's
  bracket; if it does, skip it and say so explicitly rather than silently
  omitting it without checking.

## Session lifecycle mapped onto tmux primitives

### Create

```
tmux -L stay new-session -d -s <name> -e STAY_SESSION_NAME=<name> [-c <cwd>] [command...]
tmux -L stay set-option -t <name> remain-on-exit on
```

`remain-on-exit` is what makes terminated-session review possible (see below).
Session names are sanitized before being handed to tmux: tmux session names may
not contain `.` or `:` or a bare newline — these are rejected or mapped (open
question for implementation: reject with a clear error, matching v1's existing
"invalid session name" validation posture rather than silently mangling the name
the user typed).

v1 also rejects other control/ESC bytes in a session name, on the grounds that
the name gets rendered raw in the listing/picker and a stray control byte (bare
ESC, CR, etc.) could corrupt that rendering or inject terminal sequences. tmux's
own restriction (`.`/`:`/newline) doesn't cover this — carry v1's broader
control-byte rejection forward too, since it's just as relevant to the new
ratatui picker's rendering safety as it was to v1's.

**Startup-failure reporting must stay synchronous and precise.** v1 fails
cleanly and immediately (no stale socket left behind) when a command is
non-executable or exits before the daemon signals readiness.
`tmux new-session -d -s <name> <command>` for a bad/non-executable command
instead creates the session and lets the pane die immediately — which, combined
with `remain-on-exit`, is indistinguishable from an ordinary terminated command
(e.g. exit 127) until someone inspects it. Decide and implement one of: (a)
probe the command for existence/executability before calling `new-session`,
matching v1's up-front check, or (b) accept the tmux-native behavior but make
sure a same-invocation `stay <name> <bad-command>` still surfaces the failure
clearly to the user immediately (e.g. by checking `pane_dead_status` right after
creation and reporting a fast failure rather than silently returning success).
Silently returning success when the command never ran is not acceptable either
way.

### Attach

```
tmux -L stay attach-session -t <name> [-f <flags>]
```

Run through `relay.rs` (see below), never a raw `exec()` into tmux, because the
single-key detach/copy-mode UX requires stay to sit in the input path.

- Normal attach: no `-f` flags — tmux's default `window-size=latest` behavior
  sizes the session to whichever client was last active, which is what a
  single-client i3wm workflow wants anyway.
- `-r` (read-only): `-f read-only` (deliberately *not* tmux's `-r` shorthand,
  which bundles `read-only,ignore-size` — we want these independently
  composable; see `-l` below).
- `-l` (low-priority): `-f ignore-size` — this client attaches without shifting
  the session's size to its own terminal.
- Both together: `-f read-only,ignore-size`.
- `-p` (pass-through, no attach): see Pass-through below — does not invoke
  `attach-session` at all.

**Multi-client resize semantics: a deliberate behavior change, not just a
mechanism swap.** v1's policy is strictly "minimum geometry across all
size-contributing clients" (every writable/non-`-l` client clamps the session
down to the smallest attached terminal), with active-owner transfer on detach.
Mapping onto tmux's `window-size=latest` instead means the session sizes to
whichever client was *most recently active*, not the smallest — a materially
different experience with multiple simultaneous attaches (one client's window
can now get resized out from under it by another attaching, rather than everyone
being clamped to the smallest common size). This is intentional per the "map
onto tmux's own flags rather than reimplement v1's exact semantics" decision,
but needs to be called out as a real UX difference, not filed away as
equivalent.

**Exit-code propagation.** This is necessary for a core, heavily-used v1 pattern
— `stay build cargo build; echo $?` reporting the build's actual exit status —
and the plan did not originally address it. `tmux attach-session`'s own process
exit code reflects *why the client left* (detached, session destroyed, etc.),
not the pane's command's exit status. The relay must, after the attached
`tmux attach-session` child exits, query the pane's `pane_dead_status`
(available once `remain-on-exit` has kept the pane around) and use *that* as
stay's own process exit code — not whatever `tmux attach-session` itself
returned.

**Reject trailing command words on an existing session (matching v1's "U1").**
`stay <existing-session-name> <command...>` must fail with a clear error
pointing at `-f`, not silently ignore the extra command words and just attach —
matching v1's explicit, tested behavior (`session.rs::create_or_attach`).

### Detach

No `detach-session` tmux command is invoked directly by the user's keystroke.
Instead the relay's FSM intercepts the configured detach byte (default `Ctrl+\`,
same as v1) and runs a one-shot side command:

```
tmux -L stay detach-client -s <name>
```

This causes the actual attached `tmux attach-session` child process (which the
relay is forwarding bytes to/from) to receive its own detach and exit on its
own; the relay's copy loop ends when that child exits, and stay's process exits
0\. No signal trickery needed on stay's side — tmux already handles "detach this
specific client" cleanly.

### Copy-mode (scrollback) entry

```
# on the configured copy-mode key (default Ctrl+Space, same as v1's scroll key):
tmux -L stay copy-mode -t <name>
```

The relay intercepts only the *entry* key as a one-shot side command, exactly
like detach — it does **not** forward the triggering keystroke itself. Nothing
else about the relay changes: every other byte, including `Esc`, keeps being
forwarded verbatim as it always was.

**Exit needs no interception at all.** tmux's default key table
(`mode-keys emacs`, tmux's own default) already binds `Escape` → `cancel` inside
copy-mode (confirmed in `man tmux`'s copy-mode key-bindings table:
`cancel (vi: q) (emacs: Escape)`). A plain, unmodified `Esc` byte forwarded
straight into the child PTY already exits copy-mode on its own. This removes an
entire class of complexity from the original design: no local `CopyMode` FSM
state, no bare-Esc-vs-escape-sequence-fragment disambiguation (the problem v1's
`client.rs::scroll_resid` exists to solve), and no risk of relay/tmux state
desync — because the relay never tracks copy-mode state in the first place. The
entry key is the only thing stay's relay needs to know about; tmux owns
everything else about the mode, exit included.

**Copy-mode is sticky — accepted as an intentional UX change from v1, not
fixed.** v1's scroll mode auto-exits back to the live view the instant new
output arrives while scrolled back, so a user never misses noticing that a
still-running session has kept producing output underneath them. tmux's native
copy-mode has no such auto-exit — it stays in copy-mode until the user
explicitly exits (Esc/`q`), no matter how much new output the pane produces
while they're scrolled back. Per the user's explicit call: accept this as a
real, documented behavior difference rather than building a mitigation (e.g. an
activity-alert hook) — keeps the relay's simplicity, at the cost of the "you
might miss new output while scrolled back" trade-off v1 specifically avoided.

### List

```
tmux -L stay list-sessions -F '<fields>'
tmux -L stay list-panes -a -F '<fields>'   # for pane_dead/pane_dead_status/pane_dead_time
```

Parsed into the same struct the picker and plain-listing renderer both consume.
Status derivation:

| v1 marker                      | Meaning                                | tmux-backed derivation                                                                                                                                                                                                                                      |
| ------------------------------ | -------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `a` attached                   | clients connected                      | `session_attached > 0`                                                                                                                                                                                                                                      |
| `d` detached                   | no clients                             | `session_attached == 0` and pane not dead                                                                                                                                                                                                                   |
| `t` terminated                 | command exited, session persists       | pane's `pane_dead == 1` (via `remain-on-exit`); exit code from `pane_dead_status`, exit time from `pane_dead_time`                                                                                                                                          |
| `b` broken / `[?]` unreachable | daemon unreachable, needs `-k` cleanup | **mostly eliminated** — tmux owns socket lifecycle, so there's no equivalent stale-but-undead state to guard against. The one remaining case is "the tmux server for the `stay` socket isn't running at all," which just means zero sessions, not an error. |

If `tmux -L stay list-sessions` fails because the server isn't running yet (no
sessions ever created), stay treats that as an empty list, not an error — this
mirrors v1's "no sessions" empty-list UX.

**Deterministic sort order.** `tmux list-sessions`' own ordering is not
guaranteed stable or meaningful across runs. v1 sorts deterministically (name,
creation-time tie-break) for both the plain listing and the picker; stay must
impose the same explicit sort itself after parsing, rather than printing
whatever order tmux happens to return.

### Kill / force-recreate

```
tmux -L stay kill-session -t <name>
```

`-f` (force recreate) = kill-session (ignore "no such session" error) then
create as above. Same interactive `kill <name>? (y/N)` confirmation UX as v1, in
both the plain CLI path and the picker's `k` binding. If the session being
force-recreated is currently `t` (terminated), print its previous exit status to
stderr before recreating — matching v1's behavior of not silently discarding
that information (`session.rs`).

### Pass-through (`-p`)

No `attach-session` call. Forwards stay's own stdin as a paste buffer rather
than through `send-keys` (which is argv-based and awkward for arbitrary/binary
input) — but **incrementally, not buffered-to-EOF**, per the user's explicit
choice to preserve v1's streaming behavior: v1 sends stdin to the session in
~8KB chunks as they arrive, so `tail -f data | stay -p session`-style continuous
piping delivers input as it's produced, not only once the source closes. A
single "read everything, then one `load-buffer`" call would silently break that
use case (nothing sent until EOF), so instead:

```
# loop until stdin EOF:
#   read up to ~8KB from stdin into a chunk
#   tmux -L stay load-buffer -b stay-passthrough -t <name> -   (chunk piped in)
#   tmux -L stay paste-buffer -b stay-passthrough -t <name> -d
```

Each chunk is loaded and pasted (with `-d` deleting the buffer immediately
after) before reading the next chunk, so a long-lived/streaming producer
delivers input continuously rather than only at the end. This costs a repeated
pair of `tmux` subprocess calls per chunk instead of one shot, which is the
accepted trade-off for keeping streaming parity with v1.

### Terminated-session post-mortem

This is v1's standout feature and the main reason a naive "just exec tmux"
rewrite would be a regression. It's fully recoverable on tmux:

1. `remain-on-exit on` is set on every stay-created session at creation time
   (see Create above), so the pane is not destroyed when its command exits.
2. The picker/listing reads `pane_dead`, `pane_dead_status`, `pane_dead_time`
   via `list-panes` to render the `t` marker with exit code and timestamp,
   matching v1's listing format.
3. Attaching to a terminated session still works (`attach-session` against a
   dead-but-remaining pane shows its final screen content, which is exactly the
   post-mortem review v1 offers) — tmux natively supports this, no extra work
   needed beyond the `remain-on-exit` option.
4. `stay -k <name>` on a terminated session kills the session as normal.

## Logging (`-L`, `-t`, `-s`)

Two different tmux mechanisms per the user's explicit choice, because they solve
genuinely different problems. `-s` needs more design than "call capture-pane at
a few boundaries" — that naive version either duplicates the entire retained
history on every capture (unbounded append growth) or misses everything that
happens while nobody is watching. Worked through properly below.

### Without `-s` (raw log): continuous, via `pipe-pane`

```
tmux -L stay pipe-pane -o -t <name> 'umask 077; cat >> <file>'
```

This is a real continuous stream — tmux itself owns the pipe process for the
lifetime of the pane, so there's no gap-while-detached problem and no polling of
any kind. `-o` toggles the pipe off if already active, so stay tracks whether
logging is already attached to a given session to avoid stacking duplicate pipes
on a second `stay -L work.log work`. This part of the design is unchanged from
the previous version of this doc.

### With `-s` (ANSI-stripped log): incremental `capture-pane`, not a snapshot dump

tmux has no continuous ANSI-stripping pipe, so this can't be "stream forever"
like the raw path — but it also can't be "dump the whole retained history every
time," which is what naive boundary-snapshotting degenerates into. Concretely:

**Frequency.** Three boundary triggers, plus periodic capture while a client is
attached:

1. **On attach-open** and **on detach** — a one-shot capture, run by the relay
   itself (it's already the process alive for detach/copy-mode key interception
   during that attach).
2. **On terminate** — a one-shot capture via a `pane-exited` hook.
3. **Periodically while attached** (default e.g. every 5s, configurable) — owned
   by the relay's own timer, since the relay is already the
   long-lived-for-the-duration-of-one-attach process this architecture has; no
   separate daemon is introduced to provide this.
4. **Honest gap, stated plainly:** a session with `-L -s` logging that runs
   *unattended* for a long stretch (created, nobody attaches, runs for hours)
   gets no interim capture at all — there is no process alive to drive one, by
   design (no stay daemon). The log only advances at creation, at terminate, and
   while someone is actually attached. This is a real trade-off versus v1's
   always-on raw stream and needs to be called out to the user, not glossed
   over: **`-s` is a "log what happened while watched, plus the final state"
   feature, not a guaranteed-complete transcript.** Anyone who needs a
   guaranteed-complete transcript should use the non-`-s` raw log instead, which
   has no such gap.

**Size management — incremental, not repeated full dumps.** Each capture only
appends what's *new* since the last one, using tmux's own history accounting
rather than re-capturing everything:

- tmux exposes `history_size` (lines currently retained), `history_limit` (cap),
  and `history_bytes` as format variables
  (`display-message -p '#{history_size}'` or via `list-panes -F`).
- stay keeps a small per-session cursor — the `history_size` value at the time
  of the last capture — persisted next to the log (e.g. a sidecar
  `<file>.offset`, or just held in the relay's memory for in-attach periodic
  captures and re-read from the sidecar for the boundary hooks, which run as
  separate short-lived processes).
- Each capture computes `new_lines = history_size_now - last_captured_size` and
  runs `capture-pane -p -S -<new_lines> -E - -t <name> >> <file>` — i.e., only
  the increment, appended. (tmux's `-S`/`-E` addressing is always relative to
  *now*, so the cursor has to be re-expressed as "how far back from the current
  bottom" on every call, not stored as a stable absolute line number.)
- **Eviction/gap detection:** if `new_lines` would exceed `history_limit` (i.e.,
  tmux has already evicted lines that were never captured — possible if the
  periodic interval is longer than how fast the pane produces `history-limit`
  lines of output), stay cannot recover the lost lines. It captures what's still
  available and writes an explicit marker into the log
  (`--- N lines lost, history evicted before capture ---`) rather than silently
  under-reporting or silently duplicating.
- To make that eviction case rare in practice, stay raises `history-limit` on
  sessions created with `-L -s` (e.g. `set-option -t <name> history-limit 20000`
  or similar, configurable) — trading tmux's per-pane memory for a wider safety
  margin between capture points. This is still a bounded cap, not unbounded
  growth: the *tmux-side* retained history is capped by `history-limit`; the
  *log file* only ever grows by genuinely new content, never by re-captured old
  content.
- `-t` (truncate) changes the semantics here rather than just the open behavior:
  with `-t`, every capture **overwrites** the file with the full current
  `-S - -E -` dump instead of appending an increment — "give me the latest full
  state," no cursor/offset tracking needed, trivially bounded by
  `history-limit`. Without `-t` (default), the file is a genuine append-only
  incremental transcript as described above.

This is real added complexity versus a naive "just call capture-pane a few
times" version of the idea, but it's what's actually needed for `-s` to produce
something worth calling a log rather than a handful of disconnected screen
dumps. Given `-s` is a secondary/lesser-used flag relative to raw logging, this
is worth flagging for a final go/no-go before implementation — the alternative
is documenting `-s` as strictly "snapshot on boundaries only, no periodic
capture, no incremental accounting" and accepting a much weaker feature for less
code.

### Log-file target security (kept, per explicit decision)

`umask 077` inside the `pipe-pane`/`capture-pane` shell command only controls
the mode of a file it *creates*; it does nothing about a pre-existing target at
that path. v1's `Logger::open` guards against a symlink or
wrong-owner/world-readable file already sitting at the log path — a
stay-specific file-write concern that "trust tmux's socket security" does not
cover (it's about a user-supplied log path, unrelated to tmux's own sockets).
Per the user's explicit decision, this hardening is kept, not dropped for
simplicity: before handing a log path to `pipe-pane` or `capture-pane`, stay
itself checks (via `lstat`, not following symlinks) that any pre-existing file
at the path is a regular file, owned by the current user, with no group/other
permission bits — rejecting with a clear error otherwise, exactly as v1 does.
This check runs in stay itself (not inside the shell command handed to tmux),
since the shell has no portable, race-free way to do this ahead of a plain `>>`.

### Log write-failure visibility

v1 surfaces a failing log write (disk full, removed media, quota exceeded) as a
one-time non-fatal warning rather than silently swallowing it. Neither
`pipe-pane`'s shell pipe nor a `capture-pane >> file` redirect surfaces a write
failure back to stay automatically — decide and implement a mechanism (e.g.
periodically checking the log file is still writable/growing, or wrapping the
shell command so a failing `cat`/redirect reports back via a sentinel) rather
than leaving log failures completely silent, which would be a real regression
from v1's behavior.

### Back-filling a log added after session creation

v1 supports adding `-L` to an *already-running* (or already-terminated) session,
and back-fills the new log with everything retained so far, so the log reads as
continuous from session start rather than only from the moment `-L` was invoked.
`pipe-pane`, attached at any point, only captures output *from then on*. To
preserve this: when `-L` is added to an existing session, run one
`capture-pane -p -S - -E -` (the full currently-retained history) into the log
file *before* starting the `pipe-pane` stream, so the log still reads as
complete from session start (bounded by whatever `history-limit` had already
evicted before this point, same limitation tmux's scrollback always has).

### Log path de-duplication and resolution

Two lesser but real details worth preserving: (1) a relative `-L` path is
resolved against the invoking client's cwd, not stay's own; (2) repeated/
aliased `-L` paths in one invocation (identical path, `..`-relative alias, or a
symlinked-directory alias resolving to the same canonical file) are
de-duplicated to a single log open, so output isn't written twice.

## Interactive picker

Rebuilt with a Rust TUI crate (ratatui + crossterm). The *interaction model*
(keybindings, behavior, data shown) matches v1; the *visual presentation*
deliberately does not, per explicit decision — this is a fresh design, not a v1
mimic:

- **Screen mode: probed, with a main-screen fallback.** The picker takes over
  the screen like `htop`/`vim`/tmux's own `choose-tree`, but whether that
  takeover uses the terminal's *alternate screen* (`ESC[?1049h`/`ESC[?1049l`) is
  decided at runtime, not from `TERM`. Some terminals — notably the Android SSH
  clients Termius and Conduit — advertise `TERM=xterm-256color` (the same string
  a desktop Alacritty or xterm sends) yet silently ignore the alternate-screen
  sequences, so a picker that unconditionally writes to the alt buffer there
  draws over existing content and never restores it on exit. `TERM` is identical
  across the working and broken clients, so it cannot be the discriminator;
  server-side terminfo (`tput smcup`) is equally useless, because it reads the
  *host's* terminfo database rather than describing what the *client* actually
  does. So the picker probes: after entering raw mode it sends
  `enter alt → move cursor → leave alt` as a single batched write, then compares
  the cursor position reported before and after (`ESC[6n` DSR). A conformant
  terminal restores the cursor on leaving the alt buffer; a terminal that
  ignored the sequences leaves it where we moved it. A terminal that gives no
  DSR reply at all is treated the same way — **no answer ⇒ main screen**,
  because main-screen mode is the universally safe option. Main-screen mode
  clears the screen, draws the picker in place, and clears again on exit,
  drawing over scrollback rather than corrupting it via a half-applied alt
  buffer. Two flags override the probe: `--alt-screen` forces the alternate
  screen (skipping the probe), and `--no-alt-screen` forces the main screen —
  useful where the probe itself is unreliable. Both are picker-only (they apply
  only when the picker is opened, i.e. no session name is given) and mutually
  exclusive.
- **Styled per ratatui convention**, not v1's plain blue-highlight-plus-`>`-
  prefix look — e.g. a bordered `List`/`Block` widget with ratatui's own
  selection-highlight styling, rather than reproducing v1's minimal rendering.
  Exact widget choice/styling is an implementation-time decision, not fixed by
  this plan beyond "use ratatui idiomatically."
- ↑/↓ move, Enter attach, `c` create, `v` view-only (`-r`), `e` rename in place
  (`tmux rename-session`), `l` low-priority attach (`-l`), `r` recreate (`-f`),
  `k` kill with `y/N` confirm, Esc cancel.
- Data source: `tmux -L stay list-sessions -F ...` + `list-panes -a -F ...`,
  polled on an interval while the picker is open (simplest correct v1 of this
  feature). Live push via tmux control-mode notifications
  (`session-created`/`session-closed`/`session-renamed` hooks, or a persistent
  `-C` control-mode connection) is a plausible future enhancement to remove
  polling latency, not required for the initial version.
- Non-TTY invocation (piped/redirected `stay` with no args) prints the same
  plain a/d/t listing as v1, then exits — no tmux dependency beyond the one
  `list-sessions`/`list-panes` call.
- **Zero-sessions case**: the picker still opens with an empty list, offering
  only `c` (create) — matching v1's "U5" behavior — rather than falling back to
  the plain listing or refusing to open.
- **Typed-ahead input preserved across the picker→attach handoff**: if the user
  types into the terminal in the moment between selecting a session in the
  picker and the relay actually taking over stdin, those bytes must not be
  silently dropped — thread any unread/residual stdin bytes from the picker's
  input loop into the relay the same way v1 threads `stdin_residual` through
  `MenuSelection` into `create_or_attach`. The same applies to keystrokes that
  arrive *during the screen probe*: the probe reads stdin byte-by-byte to catch
  the DSR reply, and any input that lands in that window is captured and fed
  into the picker's input reader rather than consumed and lost.

## Config & CLI

Keep the TOML config + env var override mechanism, same override precedence as
v1 (env > config file > default) and the same config file location convention,
restated precisely rather than left as "same as v1":

- **Linux**: `$XDG_CONFIG_HOME/stay/config.toml`, else
  `~/.config/stay/config.toml`.
- **macOS**: `~/Library/Application Support/stay/config.toml` unconditionally —
  `XDG_CONFIG_HOME` is a Linux/freedesktop convention and is deliberately *not*
  honored on macOS, matching v1 and the `dirs` crate's own platform split.

Full option set, cross-checked line-by-line against every field in v1's
`Config`/`TomlConfig` structs so nothing is silently dropped without saying so:

```text
# ~/.config/stay/config.toml
default_command = "bash"
detach_key = "Ctrl+\\"
copy_mode_key = 'Ctrl+Space'
history_lines = 10000
```

No directory-path override for the tmux socket location (v1's `socket_dir` /
`STAY_SOCKET_DIR`) — confirmed with the user as unneeded. `tmux -L stay ...` (a
named socket under tmux's own default per-user runtime directory) is the only
mechanism; nothing configurable here.

- **`history_lines` / `STAY_HISTORY_LINES`** — **was missing from the first
  draft of this plan; added now.** Maps directly onto tmux's own `history-limit`
  session option, applied at session-creation time
  (`set-option -t <name> history-limit <n>`). v1 accepts the literal string
  `"unlimited"` (mapped to `usize::MAX`); tmux's `history-limit` has no infinite
  sentinel — it's always a concrete, if very large, integer. `stay` maps
  `"unlimited"` to a large finite ceiling (needs picking — e.g. `1_000_000`
  lines — documented as an approximation of unlimited, not literal infinity)
  rather than silently rejecting the value v1 accepts.
- **`history_bytes_cap` / `STAY_HISTORY_BYTES_CAP`** — **dropped, with no tmux
  equivalent, and this needs to be said explicitly rather than quietly
  disappear.** It existed in v1 because stay owned an in-process scrollback
  buffer that needed a hard memory ceiling independent of line count (a handful
  of very long lines could otherwise exhaust memory even under a generous line
  cap). In v2, tmux owns its own scrollback memory internally and exposes no
  byte-based cap — only `history-limit` (line count). There is nothing for this
  option to configure. Document the removal in the v2 README/changelog the same
  way `STAY_CAPTURE_KEY`'s removal is documented, so someone carrying over an
  old config file gets a clear "no longer applicable" story rather than a
  silently ignored setting.
- `STAY_DETACH_KEY`, `STAY_COPY_MODE_KEY`, `STAY_CMD`, `STAY_HISTORY_LINES` env
  var overrides, same precedence as v1.
- `STAY_SESSION_NAME` still set for the running command, but now via tmux's own
  `-e STAY_SESSION_NAME=<name>` at `new-session` time instead of stay setting it
  before an `exec()` — same effect, simpler mechanism.
- **`STAY_SOCKET` (env var v1 exposes to the running command, pointing at its
  own session socket) has no direct v2 equivalent, and is superseded rather than
  ported:** tmux already sets `$TMUX` (`socket_path,pid,session_id`)
  automatically in every pane's environment. Any script that wants to introspect
  or talk to its own session's tmux server already has what it needs via `$TMUX`
  — stay does not need to duplicate this.
- **Dropped:** `STAY_CAPTURE_KEY` / the diagnostic capture-bundle feature. It
  existed to debug v1's custom renderer's resize/redraw bugs; there is no custom
  renderer in v2 to debug, so this has no equivalent and is removed rather than
  ported.
- Prompt integration (`--prompt-integration`) is unchanged in spirit — same
  `STAY_SESSION_NAME`-driven shell snippet, ported as-is.
- **`default_command` fallback is `$SHELL`, not a hardcoded default.** The TOML
  example above shows `default_command = "bash"` for illustration only; the
  actual unconfigured fallback (no env var, no config file entry) must be
  `$SHELL` (falling back further to `/bin/sh -i` if even that's unset), matching
  v1 exactly — not a hardcoded `"bash"`.
- **Detach-key vs. copy-mode-key collision validation.** v1 validates that its
  configurable intercepted keys (detach/scroll/capture) can't collide with each
  other, failing with a clear error naming both keys. v2 keeps two configurable
  intercepted keys (`detach_key`, `copy_mode_key`); the same validation is
  needed — reject at config-load time if they resolve to the same control byte,
  rather than letting one silently shadow the other at runtime.

### CLI flag validation

"Free to evolve" (see Decisions table) means v2's exact flag set isn't bound to
v1's, but it does **not** mean no validation policy — v1's `Cli::validate`
enforces a real combination matrix (e.g. `-t`/`-s` require `-L`; `-k` is
exclusive of the other action flags; `-r`/`-p` are mutually exclusive; action
flags require a session name; `--prompt-integration` is exclusive of everything
else) so nonsensical combinations fail fast with a clear message instead of
silently doing something unintended. v2 needs its own version of this matrix,
sized to whatever its final flag set turns out to be — worth writing explicitly
once the flags are finalized, not left implicit.

## Security model

No peer-UID verification layer, no elevated-privilege check, no stale-socket
quarantine logic — all deliberately dropped per the "trust tmux" decision.
tmux's own socket directory is already `0700` and per-UID (`$TMPDIR/tmux-$UID`
or platform equivalent), and is a mature, widely-audited piece of software.
stay's only responsibility here is not weakening that (e.g., never widening
permissions on anything it touches, never passing untrusted data into a shell
string it builds for tmux — session names and log paths must be passed as
separate argv elements to `Command`, never interpolated into a shell command
string).

## Robustness against a wedged tmux server

v1 invested heavily in never letting a single invocation hang forever against an
unresponsive daemon — a non-blocking connect-with-timeout wraps every socket
probe, so a wedged server (full accept backlog, deadlocked) degrades to a
clearly reported `[?]`/broken state instead of hanging every future `stay`
command. The plan so far assumes every `tmux -L stay <subcommand>` call returns
promptly, without stating what happens if the tmux server for the `stay` socket
itself becomes unresponsive (extremely unlikely given tmux's maturity, but not
impossible — e.g. wedged under a kernel/cgroup freeze, or an exhausted disk
during a control operation). Needs a decision: either (a) wrap every `tmux`
subprocess call with a timeout (`wait` with a deadline, killing the child and
reporting a clear "tmux unresponsive" error past it) matching v1's defensive
posture, or (b) explicitly accept the risk given how mature/stable tmux is and
document that a wedged tmux server is out of scope for v2 to defend against.
Either is defensible; leaving it unstated is not.

## Dependencies

- `clap` — CLI parsing (same as v1).
- `ratatui` + `crossterm` — picker UI (new).
- `serde` + `toml` — config (same as v1).
- `nix` / `libc` — PTY allocation + raw mode + `TIOCSWINSZ` for the relay (same
  primitives v1's `pty.rs`/`terminal.rs` already use — this part is genuinely
  reused, just aimed at a `tmux attach` child instead of the user's shell
  directly).
- **Removed entirely:** any ANSI/VT parsing crate, and all of v1's hand-rolled
  `grid`/`scrollback`/`protocol` code.

## Testing strategy

tmux is a hard runtime dependency already, so tests should run against a real
local tmux server rather than mocking the CLI surface — this matches v1's own
testing philosophy (`~/stay/stay.old/dev_docs/design/testing.md`: prefer real
PTYs over mocks — a surviving design doc, safe to consult per the note above).
Each test spawns its own isolated `tmux -L stay-test-<unique>` server so tests
can run in parallel without colliding, and tears it down (`kill-server`) on
completion/panic.

Suggested test areas (mirroring v1's `tests/` layout where the behavior carries
over):

- `session_lifecycle.rs` — create/attach/kill/force-recreate, listing output,
  name validation/sanitization.
- `terminal_attach.rs` — relay PTY sizing/resize (WINCH forwarding), raw-mode
  entry/exit, detach-key interception.
- `copy_mode.rs` — new: copy-mode entry key triggers `tmux copy-mode`; a plain
  `Esc` byte (forwarded, untouched) exits it via tmux's own native binding — no
  stay-side state or translation logic to test beyond "entry key fires the side
  command and is not itself forwarded."
- `terminated_sessions.rs` — new: `remain-on-exit` wiring, `pane_dead*` parsing,
  exit-code/timestamp display, attach-for-review, kill.
- `logging.rs` — `pipe-pane` raw streaming; for `-s`, the incremental
  `capture-pane` cursor math (offset sidecar, relative `-S`/`-E` addressing
  recomputed each call), the periodic-while-attached timer, the boundary hooks,
  eviction/gap-marker detection, and `-t`'s overwrite-instead-of-append
  semantics.
- `attach_modes.rs` — `-r`/`-l`/`-p` flag → tmux flag/command mapping, including
  `-p`'s incremental chunked streaming (not just buffered-to-EOF).
- `cli_validation.rs` — flag/name/config validation (ported from v1), including
  the new flag-combination matrix and detach/copy-mode key collision check.
- `exit_status.rs` — new: `pane_dead_status` is correctly propagated as stay's
  own process exit code after an attach, distinct from whatever
  `tmux attach-session` itself returned.
- `relay_safety.rs` — new: SIGTERM during attach detaches cleanly, SIGPIPE
  doesn't kill the relay, a relay panic still restores cooked terminal mode,
  abrupt SIGKILL of the relay doesn't leave a dangling tmux client attached, and
  (once decided) the DEC-private-mode restore bracket if tmux's own client turns
  out to need one.
- `log_security.rs` — new: pre-existing symlink/wrong-owner/world-readable log
  targets are rejected before `pipe-pane`/`capture-pane` write to them; `-L`
  added to an already-running session back-fills retained output;
  duplicate/aliased `-L` paths de-duplicate to one open.

## Suggested milestones

1. ~~**Skeleton** — Cargo project, clap CLI shape and its flag-combination
   validation matrix, tmux presence/version check (fail clearly if missing or
   too old), config loading, name validation (tmux's `.`/`:`/newline restriction
   plus v1's broader control-byte rejection).~~
2. ~~**Core lifecycle (no key interception yet)** — create/attach/kill/
   force/plain-list (with deterministic sort) working end-to-end by shelling to
   tmux; the initial attach path is a temporary native client (prefix-based
   detach only) to get something usable fast. Includes startup-failure reporting
   for a bad/non-executable command, and rejecting trailing command words
   against an existing session (v1's "U1").~~
3. ~~**Thin relay** — replace the native client with `relay.rs`: PTY allocation
   (with `setsid`/`TIOCSCTTY` on its own child), byte-forwarding loop, WINCH
   propagation, single-key detach via `detach-client` side call, SIGTERM/
   SIGPIPE handling, panic-safe terminal restoration, and exit-code propagation
   from `pane_dead_status`. This milestone carries most of the relay-safety work
   identified as missing from the first draft of this plan — treat it as the
   milestone most worth extra review time.~~
4. ~~**Copy-mode UX** — add the copy-mode-entry key interception to the relay (a
   one-shot `tmux copy-mode` side call, same shape as detach); exit is already
   free via tmux's native `Escape` binding, nothing further to build.
   Confirm/accept the sticky-copy-mode behavior difference from v1.~~
5. ~~**Picker** — ratatui interactive list with the full v1 keybinding set,
   including the zero-sessions create-only case and preserving typed-ahead input
   across the picker→attach handoff.~~
6. **Polish** — prompt integration, config/env var naming finalization, docs
   (README rewritten for the new architecture, dropping all mention of the
   custom PTY/ANSI engine).
7. **Terminated sessions** — `remain-on-exit`, `pane_dead*` parsing, `t`-status
   listing with exit code/time, review-by-attach, printing prior exit status
   before a `-f` force-recreate.
8. **Attach-mode flags** — `-r`/`-l`/`-p` mapped onto tmux flags/buffer
   commands, including `-p`'s incremental chunked streaming.
9. **Logging** — `pipe-pane` raw path for `-L`; the incremental `capture-pane`
   cursor/offset design, periodic-while-attached timer, boundary hooks, and
   eviction-gap marker for `-s`; `-t`'s overwrite semantics for both paths;
   pre-write log-target security checks (symlink/owner/permission rejection);
   back-filling a log added to an already-running session; path de-duplication.

## Open risks / edge cases to keep in view

- **Session name sanitization**: tmux rejects `.`/`:`/newline in session names;
  decide reject-with-error (consistent with v1's existing validation posture)
  vs. silent mapping before implementation starts.
- **`-L`/log file permissions**: `pipe-pane`'s shell command runs via tmux's own
  shell invocation — creating the log file at `0600` needs the command stay
  hands to `pipe-pane` to pre-create/chmod the file itself (e.g.
  `sh -c 'umask 077; cat >> "$0"' <file>`) rather than relying on shell
  redirection defaults, to preserve v1's `0600`-created-log guarantee.
- **`-s` offset sidecar durability**: the `<file>.offset` cursor is itself state
  that can go stale or missing (deleted by the user, moved log file, disk full
  mid-write) — decide the recovery behavior (treat missing/corrupt offset as
  "capture everything currently retained, starting fresh" is the safe default)
  before implementation, and make sure the offset file is written atomically
  (temp file + rename) so a crash mid-update can't corrupt it into an unusable
  state.
- **`-s` periodic-capture interval vs. history-limit sizing**: the default
  interval and the raised `history-limit` for `-L -s` sessions need to be chosen
  together — too long an interval relative to how much output the command can
  produce risks tripping the eviction-gap case even with a raised limit; needs a
  sensible default informed by realistic output rates, not picked arbitrarily.
- **tmux server exit on last session killed**: not an error state — listing code
  must treat "no server for this socket" identically to "zero sessions."
- **Version gating**: confirm the exact minimum tmux version needed for every
  feature used here (`ignore-size` client flag, `pane_dead_status`/
  `pane_dead_time` format variables, `remain-on-exit`) before finalizing the
  "tmux ≥ 3.2" claim — verify against tmux's CHANGES file per feature rather
  than assuming.
- **No equivalent to v1's SIGUSR1 "recreate socket if deleted" recovery**: if a
  user manually deletes tmux's own server socket file while sessions are
  running, there's no stay-side recovery mechanism (tmux itself would need
  restarting, at the cost of attach-ability though not the sessions' in-memory
  state). Low severity given how rarely this happens and how far outside stay's
  control tmux's own socket file is, but worth a one-line mention in the
  eventual README rather than silent surprise.
- **Test-infra orphan reaping**: each test spawning its own
  `tmux -L stay-test-<unique>` server needs teardown on panic (already planned),
  but a hard-killed test binary (CI timeout, SIGKILL) can still leak a running
  orphaned tmux server behind. v1's test harness sweeps PID-liveness-gated
  orphans left by a previous abnormal run; v2's harness needs the same sweep for
  its per-test tmux servers, or orphaned test servers accumulate silently across
  CI runs.
