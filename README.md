<p align="center">
  <img src="assets/transparent.png" width="320" alt="scytty hooded reaper and terminal logo">
</p>

# scytty


A minimal native terminal in Rust: GPUI draws the window,
libghostty-vt 1.0.1 (via `ghostty-vt` 0.9) interprets terminal output, and
`portable-pty` runs your login shell. This uses Ghostty's VT library,
not the full Ghostty application embedding API.

## Workspace

- `crates/gui/` — `scytty`: GPUI client, terminal rendering, SSH transport and system font discovery
- `crates/server/` — `scytty-server`: sessions, tabs, PTYs and server-side Ghostty state
- `crates/common/` — `scytty-common`: compressed binary protocol and local connection bootstrap

The GUI does not depend on the server crate; both share only `scytty-common`.

## Run

Requires Rust 1.96+, Zig 0.17.x on PATH (or set `ZIG`), a C/C++ build
toolchain, and internet access for the first build. The Ghostty binding
build downloads and compiles its pinned upstream source. macOS also requires
Xcode command-line tools and a Metal-capable GPU. GPUI compiles Metal shaders at
runtime, so the optional Metal compiler toolchain is not required.

```sh
cargo build --release --workspace  # build GUI and server
cargo run -p scytty --release         # create or attach the "default" session
cargo run -p scytty --release -- connect --session work # create or reattach named session
```

CLI: `scytty connect [--host HOST] [--session NAME] [--bin EXECUTABLE]`.
Omit `--host` for a local connection and `--session` for `default`. Bare `scytty`
opens the default local session. `--bin` selects the executable for a new session,
not a shell command or argument list; quote paths containing spaces. Existing
sessions keep their program; an explicit different `--bin` is rejected.
Use `scytty --help` for usage. The old `--ssh` and positional-session forms have
been replaced by `connect` and named options.

Locally, the GUI launches `scytty-server --stdio` from beside the GUI executable
or PATH, using the same stdio bridge as SSH. The bridge connects to the private
Unix socket and starts the persistent daemon if needed.
Window size and position are saved on normal exit and restored on launch.
The last saved geometry is shared across sessions; configured dimensions are
used when no saved geometry exists. GPUI stores geometry in
`$XDG_STATE_HOME/scytty/window` (default `~/.local/state/scytty/window`);
the former egui geometry is not imported.

One window can attach to each session.
Every shell started by the server receives `SCYTTY_SESSION` set to its session name
(including over SSH).
Closing a local or SSH window detaches and preserves its session and shells. Reopen the
same session name (`default` when omitted) to restore its tabs and terminal snapshots.
Closing the last tab deletes the session and closes the window. After an established
connection drops, the GUI automatically reconnects to the same local or SSH session,
retrying after 1, 2, 4, 8, 16, then 30 seconds (capped). The last screen stays visible
with a reconnect banner; keyboard input is discarded while disconnected, never
buffered or replayed. Reattachment restores fresh terminal snapshots. Initial
connection failures, rejected sessions, and incompatible servers still show an error;
reconnect never approves a daemon restart or takes over another attached window.
For an already-attached session, **Disconnect previous and connect** explicitly
transfers it to this window without ending its tabs or shells. The previous window
shows a disconnection message and cancels automatic reconnect. It offers
**Reconnect** (normal attachment, only if available) and **Disconnect previous and
connect** (explicit takeover). An already-attached rejection offers the same
buttons and stops retries; clients never automatically take the session back.
This requires protocol 8 on both client and server. Upgrade/restart the daemon only
when safe: restarting the server ends existing sessions.
A normal disconnect releases the session's attachment immediately. Clients send
heartbeats every 10 seconds; after about 30 seconds without client traffic, the
server releases a stale attachment while preserving its tabs and shells. The GUI
also reconnects if server replies stop for 30 seconds. Suspending a client longer
than this can detach it; background/minimized windows still send heartbeats.
The socket and server log live in `~/.scytty`. Sessions do not survive server restart;
if the session no longer exists, reattachment creates a new shell.

The default font is the system **monospace** family at 16pt. No fonts are bundled.
Install and select a Nerd Font to display the status bar's icons.

## Just commands

```sh
just build          # release GUI + local/embedded remote servers; macOS also creates target/release/Scytty.app
just run            # build and launch GUI without installing; accepts connect --host / --session / --bin arguments
just install        # build and copy the macOS app to ~/Applications
just build-server   # native release server only
just build-servers  # bundle Linux x86_64/ARM64; on macOS, also macOS ARM64/x86_64
just build-linux    # Linux x86_64 server, using Zig (glibc 2.28+)
just install-linux  # cross-build and install to 192.168.1.2:~/.local/bin
```

`just build` / `just build-servers` install the needed Rust targets and require
Zig on PATH. Linux payloads are statically linked with musl; macOS payloads can
only be built on macOS. `just build` embeds everything in `target/server-bundle`.
For a GUI-only development build with existing payloads:

```sh
SCYTTY_SERVER_BUNDLE="$PWD/target/server-bundle" cargo build -p scytty
```

Plain Cargo GUI builds without this variable have no remote payloads and report
that limitation when connecting. Windows developers can populate this directory
from the workflow's `server-bundle-*` artifacts before building.

The older `just build-linux` / `just install-linux` commands remain available for
manual glibc 2.28+ deployment; they require the `x86_64-unknown-linux-gnu` Rust
target. Manual installation does not restart the remote daemon.

## GitHub Actions builds

The `Build` workflow runs only when a tag is pushed. After all builds and tests
succeed, it creates a GitHub Release for that tag with downloadable archives:

- macOS ARM64: desktop `.app` bundle; ARM64 and x86_64 standalone servers.
- Linux x86_64 and ARM64: statically linked musl server archives.
- Windows x86_64: GUI and matching local server executable archive (ConPTY).

Extract the desktop archive and move `Scytty.app` into Applications. Its matching
server is bundled inside, so no separate server installation is needed locally.
The app is ad-hoc signed, **not notarized**; macOS Gatekeeper may block it until
explicitly allowed in System Settings.

On Windows 10 version 1809 or later, extract both executables into the same
folder and run `scytty.exe` for a local session. The server starts automatically
and sessions survive closing the GUI. Run `scytty.exe connect --host user@host --session work`
for remote Unix sessions; OpenSSH (`ssh.exe`) must be on PATH, with no manual
remote server installation needed. Both desktop archives embed all four
remote server variants. To build locally, use Rust, Zig and the MSVC build tools;
set `SCYTTY_SERVER_BUNDLE` as described above to include remote payloads.

Publish a release with `git tag v0.1.0 && git push origin v0.1.0` (use a new tag
for each release).

## Configuration

The GUI evaluates `~/.config/scytty/config.lua` with bundled **LuaJIT** on startup
(or `$XDG_CONFIG_HOME/scytty/config.lua`). Return a table:

```lua
return {
  font_size = 16,
  default_cursor_style = "block",
  background_opacity = 0.7,
  window_width = 960,
  window_height = 640,
}
```

See `config.example.lua`. Missing options use defaults; unknown options and
invalid values are errors. Config applies to terminal and tab-bar fonts.
Set `font_family = "Menlo"` (or another installed family name) to select a font.
The default `"monospace"` selects the system monospace family. The regular face
is used; unknown primary families fall back to the system monospace font.
`font_family` also accepts an ordered glyph-fallback array, for example
`font_family = {"Menlo", "FiraCode Nerd Font Mono"}`. The first family sets the
terminal metrics; later families are passed to GPUI's native shaper for missing
glyphs, including icons, in both terminal text and the UI. Native fallback support
is platform-dependent. This does not synthesize missing italic faces.
No bundled glyph fallback is
provided. Choose a monospace family for correct terminal alignment.
Restart the GUI to reload. Only load trusted config: this is executable Lua.

Customize the native window title with `format_window_title = function(ctx)
return ctx.session .. " - " .. ctx.title end`. It receives `session`, the active
tab's `title`, and `remote` (boolean), and must return a string.
Server settings are not configurable yet.

Modules beside `config.lua` work with `require()`, including `module/init.lua`.
For example, `config.format_tab = require("format-tab-title")` loads
`~/.config/scytty/format-tab-title.lua`, which should return a callback. Settings
modules can return a function and apply it with `require("settings")(config)`.
No working-directory change or WezTerm API shim is required.

The bar is a single transparent monospace text row, with clickable tabs and `+`.
When tabs overflow, the active tab and its immediate neighbors stay visible;
long titles are clipped as needed to fit up to three tabs. The selected tab is not
at the visible edge unless it is the first or last tab. `<` and `>` appear only when tabs
are hidden on that side; clicking an indicator activates the nearest hidden tab.
The new-tab button stays outside the scrolling strip. Customize the three labels
in `config.lua`, then use **Reload config**:

```lua
tab_left_indicator = {text = "←", color = "#cba6f7"},
tab_right_indicator = {text = "→", color = "#cba6f7"},
new_tab_button = {text = "＋", color = "#a6e3a1"},
```

Defaults are `<`, `>`, and `+` with inherited text color. Each setting accepts a
plain string or `{text = "…", color = "#RRGGBB"}`; color is optional. Text must be
nonempty and single-line. Unicode symbols depend on your installed font.
Nerd Font icons followed by a space (e.g. `" "`) use `icon_scale`, just like tab
and status text; the space is consumed to reserve the wider icon cell.
Menu-entry labels use the same rule, e.g. `label = " New tab"`; only the icon is
scaled, while menu text stays at its normal size.
Hidden indicators take no space. Tab formatting and button actions are unchanged.
It occupies the titlebar area beside the native macOS window buttons; drag empty
bar space to move the window. The native title text is hidden.
Defaults show plain tab titles (white active, gray inactive), a 25-character
tab limit, and empty left/right status. Override `left_status`, `format_tab`, or
`right_status` with Lua functions (see `config.example.lua`). Each receives a table
with `remote` and `user_vars` (the active tab's OSC 1337 variables);
`format_tab` also receives `title`, `index` (1-based), and `active`, and its own
 tab's `user_vars`.
Return a string or an array of `{text = "...", color = "#RRGGBBAA"}`
segments. Color is optional. Nerd Font icons followed by a space enlarge into
that space automatically. Set `icon_scale = 1.0` (default; range 1–3) to configure their
enlargement in both the terminal and status bar. Icons remain clipped to the row.
Custom command icons, themes and status animations belong in your Lua config;
Rust has no ghost-specific rendering logic.
Callbacks run on GUI redraw; the GUI refreshes once a second for clocks.
This is a scytty API, not a WezTerm compatibility layer. To display shell status,
your `right_status` callback can return `ctx.user_vars.right_status or ""`.
Variables are set with `OSC 1337;SetUserVar=NAME=BASE64_VALUE`, terminated by BEL
or ST; they survive detach/reattach and work over SSH. Limits: 64 variables per
tab, 128-byte alphanumeric/underscore names and 4096-byte UTF-8 values. ANSI foreground colors in status text support
16-color, 256-color and truecolor SGR sequences; reset restores the segment's
Lua color. Other terminal controls and styling are ignored.

```sh
printf '\033]1337;SetUserVar=right_status=%s\007' "$(printf 'building ' | base64 | tr -d '\n')"
printf '\033]1337;SetUserVar=right_status_timestamp=%s\007' "$(date +%s | tr -d '\n' | base64 | tr -d '\n')"
```

Progress uses OSC 9;4 and is exposed to the same callbacks as `ctx.progress`:
`nil` when cleared, otherwise `{state = "normal", percent = 42}`. States are
`normal`, `error`, `indeterminate`, and `paused`; `percent` is nil when omitted.
Status callbacks receive the active tab's progress; `format_tab` receives its
own tab's progress. It survives detach/reattach, including over SSH. The active
tab also updates native icon progress automatically: each Windows window's taskbar
button, or the macOS Dock icon for the most recently focused window. Switching
tabs, clearing progress, disconnecting, or closing the window clears stale progress.
Windows shows native normal/error/paused/indeterminate states; macOS uses a
percentage bar or a moving segment for indeterminate work, without distinct
error/paused colors.
Lua status text remains opt-in, for example:

```lua
right_status = function(ctx)
  local p = ctx.progress
  if not p then return "" end
  return p.state .. (p.percent and (" " .. p.percent .. "%") or "")
end,
```

```sh
printf '\033]9;4;1;42\007' # normal, 42 percent
printf '\033]9;4;0\007'    # clear
```

### Program status (OSC 7501)

`format_tab`, `left_status`, and `right_status` also receive `ctx.state`:
`nil` with no reports, otherwise the
most recently updated surviving record's state (`idle`, `working`, `done`,
`blocked`, or `error`). This is not an aggregate of concurrent tasks.
`ctx.program_status` contains all reports keyed by ID; `""` is the root record.
Each has `state` and optional `app`, `kind`, `progress`, `title`, and `message`.
For example:

```lua
format_tab = function(ctx)
  return ctx.title .. (ctx.state and (" [" .. ctx.state .. "]") or "")
end,
```

```sh
printf '\033]7501;state=working:app=cargo:progress=40\007'
printf '\033]7501;state=blocked:id=build:kind=permission:msg=QXBwcm92ZT8=\007'
printf '\033]7501;state=clear\007'
```

`title` and `msg` on the wire are base64 UTF-8; Lua receives decoded plain text.
Reports replace their whole record; omitted fields disappear. A missing `app`
inherits from the nearest ancestor. Clearing an ID also clears its descendants;
clearing without an ID or a terminal reset clears everything. New shell prompts
(OSC 133 A) discard idle/working/blocked records; done/error records remain until
cleared by the sender or reset. Closing a tab removes its records. Each tab keeps
at most 256 records, evicting the least recently updated. State survives
reattachment, including SSH. Support queries are answered by the server.
These are untrusted program-provided values; no badges, notifications, or
OSC 9;4 progress updates are triggered automatically.

This changes tab metadata on the wire: update both GUI and server and restart
the daemon when safe (restarting ends existing sessions).

## Configurable shortcuts

Add `key_bindings` to `config.lua`, then use **Reload config**:

```lua
key_bindings = {
  { keys = {key.SUPER, key.SHIFT, key.N}, actions = {"new_window"} },
  { keys = {key.SUPER, key.T}, actions = {} }, -- Pass through to the terminal.
  { keys = {key.SUPER, key.R}, actions = {"reload_config"} },
}
```

Entries override the defaults; omitted shortcuts keep their existing behavior.
Scytty supplies the Lua `key` namespace. Modifiers are `key.SUPER`, `key.CTRL`,
`key.ALT`, `key.SHIFT`, and `key.FN`. Super means Command on macOS and Windows/Super
elsewhere, not Ctrl. Keys include `key.A`–`key.Z`, `key.DIGIT0`–`key.DIGIT9`,
`key.F1`–`key.F35`, `key.UP`/`DOWN`/`LEFT`/`RIGHT`, `key.PAGE_UP`/`PAGE_DOWN`,
`key.HOME`, `key.END`, `key.ENTER`, `key.ESCAPE`, `key.TAB`, `key.SPACE`,
`key.BACKSPACE`, `key.DELETE`, `key.INSERT`, and punctuation such as `key.PLUS`,
`key.MINUS`, `key.EQUAL`, `key.SLASH`, and `key.BACKSLASH`.
Use exactly one non-modifier key per chord. For shifted punctuation, use the
printed key (e.g. `{key.SUPER, key.PLUS}`), not a guessed physical key or an extra
consumed Shift modifier. Custom bindings match exact modifiers and ignore the
generated text. Key sequences and modifier-only bindings are not supported.

Actions: `new_tab`, `new_window`, `close_tab`, `reload_config`, `copy`, `paste`,
`find`, `previous_tab`, `next_tab`, `previous_prompt`, `next_prompt`, `zoom_in`,
`zoom_out`, `reset_zoom`. Actions run in order, stopping at the first error;
completed actions are not rolled back. Server requests are queued, not awaited:
`new_tab` followed by `paste` may still paste into the current tab.
`close_tab` ends the tab's shell/processes. Empty `actions = {}` bypasses the
default action and uses the terminal's normal keyboard encoder. Duplicate chords,
unknown key constants, and malformed arrays reject the config; a failed reload
keeps the current configuration.

Bindings apply to the terminal view. The open menu and search editor retain their
own navigation/editing keys; the configured `find` shortcut can select the current
search query again.

## Default shortcuts

macOS/Linux use **Super** (Command on macOS). Windows uses Ctrl-based defaults
to avoid reserved Windows-key shortcuts. Custom bindings can still use any modifier.
The OS or window manager may intercept some Super combinations before Scytty receives them.

- **Super+F** (**Ctrl+Shift+F** on Windows): search the active tab's buffer,
  including scrollback. **Enter / Shift+Enter** moves to the next/previous match;
  **Escape** closes search. Matching is literal with full, locale-independent Unicode
  case folding (`Straße` matches `STRASSE`, and `Σ`, `σ`, `ς` match). Matches map
  back to whole original cells, never half a folding expansion; no normalization
  is applied. Search runs locally, including for SSH sessions,
  and leaves the terminal's text selection intact.

- **Super+Plus / Super+Minus** (**Ctrl+Plus / Ctrl+Minus** on Windows): increase/decrease font size
- **Super+0** (**Ctrl+0** on Windows): reset to the configured font size
- **Drag**: select terminal text
- **Super+Up / Super+Down** (**Ctrl+Shift+Up / Ctrl+Shift+Down** on Windows):
  jump to the previous/next shell prompt (OSC 133)
- **Super+Shift+Click** (**Ctrl+Shift+Click** on Windows):
  select the clicked command's output (OSC 133)
- **Scrollbar**: drag the right-hand thumb or click its track to navigate scrollback
- **Super+C / Super+V** (**Ctrl+Shift+C / Ctrl+Shift+V** on Windows):
  copy selection / paste clipboard (including over SSH)
- **Super+T** (**Ctrl+T** on Windows): new tab
- **Super+Left / Super+Right** (**Ctrl+Left / Ctrl+Right** on Windows): previous/next tab, wrapping at the ends

Tab text and the `+` character also work with the mouse. Right-click anywhere on the top bar for a
menu (Up/Down and Enter select; Escape or an outside click dismisses it).
Configure entries in `config.lua`:

```lua
return {
  plus_menu = {
    { label = "New tab", action = "new_tab" },
    { label = "Close tab", action = "close_tab" },
    { label = "New window", action = "new_window" },
    { separator = true },
    { label = "Reload config", action = "reload_config" },
    { label = "Production", action = "connect", host = "user@server", session = "work", bin = "/usr/bin/fish" },
  },
}
```

Use `{ separator = true }` for a non-clickable divider; Up/Down skips dividers.
A menu containing only separators does not open.

The default menu contains New tab, Close tab, New window and Reload config.
Close tab ends the active tab's shell/processes; closing the last tab closes the window.
`plus_menu = {}`
disables it. Reload config re-reads Lua and its modules for the current window,
applying font, opacity, status, menu and scrollback settings without reconnecting.
Invalid config keeps the previous settings. Window dimensions apply only to new
windows. Lowering the scrollback limit may discard older history.
New window creates a fresh session on the current host. Connect opens a separate
window; its session defaults to `default`. An already-attached session offers
**Disconnect previous and connect**, or you can close its existing window first.
Optional `bin` selects an executable
path or name on the server (not a shell command or argument list). It is used for
the new session and all its new tabs; omitted `bin` keeps the server's default
shell or an existing session's program. An explicit different `bin` cannot replace
an existing session's program: choose another `session` name instead.

Applications can send native desktop notifications using OSC 9 or OSC 777,
including from SSH sessions:

```sh
printf '\033]9;Job finished\007'
printf '\033]777;notify;Build;Build finished\007'
```

Notifications are shown by the attached GUI, including for inactive tabs. Allow
Scytty notifications in your OS settings; on macOS use the installed Scytty.app.
Requests are limited to one every two seconds across GUI windows, with bounded
text and queue sizes. Windows registers a per-user Scytty notification identity.
Click-to-focus actions and Kitty OSC 99 notifications are not implemented.

Applications can copy UTF-8 text to the local system clipboard using OSC 52 or
Kitty OSC 5522, including from SSH sessions. Writes/clears are enabled; clipboard
reads are denied (remote programs cannot retrieve your clipboard). Text is limited
to 8 MiB. Other MIME types and primary/selection clipboards are not supported and
leave the system clipboard unchanged. OSC 5522 protocol acknowledgements confirm
terminal acceptance, not successful OS clipboard delivery.

```sh
# OSC 52: copy "hello"
printf '\033]52;c;aGVsbG8=\007'
# OSC 5522: begin, send text/plain, commit
printf '\033]5522;type=write\033\\'
printf '\033]5522;type=wdata:mime=dGV4dC9wbGFpbg==;aGVsbG8=\033\\'
printf '\033]5522;type=wdata\033\\'
```

ANSI underline (single, double, curly, dotted and dashed) and strikethrough are
rendered across the terminal cell grid, including spaces and wide characters.
Underline colors support SGR 58 (palette or RGB); SGR 59 restores the text color.
Application-controlled cursors support block, underline and bar shapes (DECSCUSR),
blinking, and hide/show (DEC mode 25). Unfocused windows show a steady hollow cursor;
solid blocks keep the character beneath them readable.
`default_cursor_style` accepts `"block"` (default), `"bar"`, `"underline"` or
`"block_hollow"`. Reload config applies it to open tabs without overriding an
application's explicit cursor shape; a cursor reset returns to the configured style.

Applications can change the terminal's default foreground/background/cursor colors
with OSC 10/11/12 and reset them with OSC 110/111/112. Background opacity still
applies. OSC 22 changes the mouse pointer; shapes unavailable in GPUI (including
busy, help and zoom) use an arrow. Link hover takes precedence over the requested
pointer. Colors and pointer state survive reattachment.

Prompt navigation and command-output selection require OSC 133 markers emitted
by the shell or its integration script. Scytty does not install shell hooks or
infer command boundaries from prompt text.

Click an HTTP/HTTPS URL or an OSC 8 hyperlink to open it in the system browser.
Links are underlined on hover and the pointer becomes a hand; dragging still selects text. Other URL
schemes are not opened. Plain URLs are detected within a visible row; wrapped
links should use OSC 8.

New tabs inherit the active tab's working directory on the server (local or SSH), using the foreground
process's directory or the shell's directory as a fallback. If neither can be
read, the server's launch directory is used.

The GUI runs locally; the server supports macOS, Linux, and Windows. On Unix,
the default shell is `$SHELL`, falling back to `/bin/sh`; on Windows it is
`%COMSPEC%`, falling back to `cmd.exe`. Windows tabs currently start in the
server's launch directory rather than inheriting the foreground process cwd.

Windows local connections use authenticated loopback TCP, with a random secret
in a user-only ACL-protected `%LOCALAPPDATA%\\scytty\\server.endpoint` file
(`%USERPROFILE%` is the fallback). Authentication uses HMAC-SHA256 with fresh
client and server nonces before the usual versioned protocol. The daemon's
`--check-daemon` and explicitly confirmed `--restart-daemon PID SHA256` commands
are supported; a restart ends all its sessions. Automatic SSH deployment still
supports only remote macOS/Linux hosts.

## Remote sessions over SSH

Use an official desktop release or `just build`, then connect:

```sh
scytty connect --host user@host --session work --bin /usr/bin/fish
# From source (builds the embedded payloads first):
just run connect --host user@host --session work
```

Scytty detects the remote OS/architecture and uploads its embedded matching
server to `~/.scytty/servers/<platform>-<sha256>/scytty-server`. An identical cached
binary is reused; uploads are private and atomically installed. Supported remote
hosts are Linux and macOS, each on x86_64 or ARM64. The host needs a POSIX shell,
standard Unix utilities, SSH access and permission to execute files under HOME;
no Rust toolchain or server on PATH is required.

Platform detection, cache selection, optional upload and the stdio bridge share
one SSH connection. Uploads use the remote system's `/usr/bin/base64` decoder.
Before uploading, Scytty checks cached servers and the running daemon's protocol.
An identical build is reused. For a protocol-compatible daemon with a different
build, choose **Connect to existing — preserve sessions** or
**Upload, restart, and reconnect — ends ALL sessions**. No upload happens before
that choice. If this client has no bundled server for the platform, a compatible
cached server can still be used. An incompatible daemon offers only
**Restart with bundled server**. **Restarting
ends ALL sessions and shells on that remote host.** Closing the window instead
leaves the daemon untouched. Confirmation rechecks the daemon PID and SHA-256
before terminating it, then the client connects and starts the bundled version.
Existing sessions are never automatically migrated or killed.

One SSH connection carries the **entire session**, including every tab's
input, output, snapshots and control messages. The system `ssh` client handles
authentication, host keys and SSH config aliases (including ports/ProxyJump).
Use an SSH agent or authenticate from the launching terminal. No TCP listener
or exposed server port is needed. The remote server starts automatically.

Closing the window or losing SSH detaches without killing remote shells.
The GUI automatically retries a lost connection. Run the same command to reattach
from a new window.
A fixed startup handshake checks protocol versions before any session frames
are decoded, locally and over SSH. Incompatible or pre-handshake peers produce
an explicit update/restart error rather than attempting to decode session data.
This is a compatibility check, not automatic version conversion. Bump
`PROTOCOL_VERSION` in `crates/common/src/protocol.rs` when message or Ghostty snapshot
formats change incompatibly. Restarting the daemon ends existing sessions.

Connection/startup and runtime errors appear in a selectable text panel with a
**Copy error** button, preserving the full error chain for diagnostics.
Deployment/cache reuse and explicit daemon replacement have automated local
tests; real-host SSH and the complete release workflow still need validation.

## Scrollback

Set `scrollback_mb = 32` in `~/.config/scytty/config.lua` to retain up to 32 MiB
per tab. Whole numbers 0–64 are accepted; 0 disables scrollback. Reopen the GUI
to apply the setting to existing and new tabs, locally or over SSH. Reducing the
limit discards older history; increasing it cannot recover already-discarded
content. Both server and GUI retain their own terminal buffers, so larger limits
increase memory usage on both sides. Protocol frames are bounded at 128 MiB to
accommodate scrollback and image snapshots.

## Images

Kitty graphics supports inline PNG and raw RGB/RGBA images, including chunked
transfers, crops, scaling, z-order, scrolling, deletion and detach/reattach.
Unicode placeholders (including inferred row/column markers) and relative
placement chains are rendered from Ghostty's resolved image geometry.
Kitty animations support timed playback, finite/infinite loops, loading-mode
streaming, stop/start and client-selected frames. Visible tabs drive playback
locally; reattaching restarts animation timing.
Use a Kitty-compatible sender with **direct/stream transfer**, also over SSH
(for example `kitten icat --transfer-mode=stream picture.png`, or add
`--unicode-placeholder` for placeholder-based positioning).

Minimal smoke test (a red block):

```sh
printf '\033_Ga=T,q=2,f=24,s=1,v=1,i=1,c=10,r=5;/wAA\033\\'
printf '\033_Ga=d,d=A\033\\' # delete
```

Image storage is capped at 16 MiB per terminal screen. PNG decoding is bounded
to 16 MiB RGBA and 4096 pixels per dimension; GPU texture limits may be lower.
File, temporary-file and shared-memory transfer are disabled: programs must send
image bytes through the PTY. Sixel and iTerm2 images are not rendered yet.
Placeholder, relative-placement and animation rendering require a GUI rebuild;
no daemon restart is needed.

## Included

- Interactive login shell, Ctrl shortcuts, navigation and F1–F12 keys
- ANSI colors, alternate screen, terminal replies and resize propagation
- Clipboard paste with Ghostty's bracketed-paste encoding
- Drop files into the window to paste shell-quoted paths (multiple files supported).
  SSH uses POSIX quoting; local Windows follows `COMSPEC` (normally cmd.exe), or
  the configured `bin` for PowerShell/POSIX shells. Unsafe cmd.exe paths containing
  `%`, `!`, or quotes are rejected rather than expanded. Unknown Windows programs
  are rejected; after reattaching a custom-shell session, specify its `bin` in the
  connect menu so quoting matches. Quoting assumes the configured shell, not a
  different interpreter started inside it. In SSH sessions these are local paths,
  not file uploads.
- Mouse-wheel scrollback, configurable with `scrollback_mb` (default 16 MiB per tab)
- Multiple tabs, server-owned PTYs, live state/output synchronization
- Gzip-compressed binary protocol over a private local Unix socket
- Bounded output queues with backpressure for image/output bursts; stalled
  socket writes disconnect after their 5-second timeout. A stalled client may
  briefly pause other sessions while the server waits for queue capacity.

## Limits

One attached window per session; no splits or session-list UI yet.
Mouse reporting supports clicks, releases, dragging, motion and both wheel axes
using the application's requested protocol. Hold Shift for local selection and
scrollback; scrollbar dragging stays local.
Terminal styling supports bold, italic, dim, blinking text, inverse, conceal,
colored single/double/curly/dotted/dashed underlines, overline and strikethrough.
Bold and italic use the configured font's available faces; text and decorations
blink together without blinking backgrounds.
Keyboard input follows Ghostty's negotiated legacy/fixterms, xterm modifyOtherKeys
and Kitty protocols, including repeat/release events, modifiers, alternate key
codes and associated text. Native IME commits remain text; shortcuts and clipboard
paste stay local. Held-key releases follow the original tab and are flushed on
window focus loss. Negotiated focus reporting follows window activation and tab
changes. Exposed keypad operators, media/browser keys and Kitty F1–F35 are encoded;
Caps Lock toggles produce balanced key events because GPUI provides no physical
Caps Lock release.
GPUI exposes logical keys, not physical scan codes, modifier sides, Num Lock state
or all keypad distinctions. Those details and layout-independent alternate codes
cannot be reported faithfully yet; shifted punctuation may also change identity
between press and release. Full physical-key support requires a GPUI change.
Unicode rendering preserves Ghostty's grapheme clusters and cell widths, including
combining marks, CJK, emoji modifiers, ZWJ sequences, flags and variation selectors.
Wide-cell tails and wrap padding do not draw duplicate glyphs; selection, copy,
search, reflow and reconnect preserve the original UTF-8 without normalization.
Search-field editing moves and deletes whole graphemes. Native IME ranges are
converted safely between UTF-16 and UTF-8, including non-BMP characters.
Glyph coverage depends on installed fonts and GPUI's platform font fallback.
Bidirectional text uses the Unicode Bidirectional Algorithm across soft-wrapped
paragraphs, with the terminal's left-to-right paragraph base. RTL runs, numbers,
isolates and mirrored punctuation retain grapheme and wide-cell boundaries.
Cursors, selection, links, search highlights and mouse reports share the same
logical/visual mapping. Stored and copied text stays in logical order, which can
differ from its displayed order. Applications that already emit visually reordered
RTL text will be reordered again. Bidi and case-folding tables follow Unicode 16.
Terminal applications can write supported text to the clipboard; clipboard reads
remain denied.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
