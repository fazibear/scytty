<p align="center">
  <img src="assets/logo.png" width="320" alt="scytty hooded reaper and terminal logo">
</p>

# scytty

A minimal native terminal in Rust: GPUI draws the window,
libghostty-vt (via `ghostty-vt`) interprets terminal output, and
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
cargo run -p scytty --release -- work # create or reattach named session
```

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
Closing the last tab deletes the session and closes the window. Unexpected
connection loss also detaches. The socket and server log
live in `~/.scytty`. Sessions do not survive server restart.

The default font is the system **monospace** family at 16pt. No fonts are bundled.
Install and select a Nerd Font to display the status bar's icons.

## Just commands

```sh
just build          # release GUI + matching local server; macOS also creates target/release/Scytty.app
just run            # build and launch GUI (installs to ~/Applications on macOS); accepts session / --ssh arguments
just build-server   # native release server only
just build-linux    # Linux x86_64 server, using Zig (glibc 2.28+)
just install-linux  # cross-build and install to 192.168.1.2:~/.local/bin
```

Linux builds require `zig` on PATH and
`rustup target add x86_64-unknown-linux-gnu`. Installation uses your system
SSH credentials and atomically replaces the binary without restarting the
remote daemon. Restart it separately when safe; restarting ends its sessions.

## GitHub Actions builds

The `Build` workflow runs only when a tag is pushed. After all builds and tests
succeed, it creates a GitHub Release for that tag with downloadable archives:

- macOS ARM64: desktop `.app` bundle plus standalone server archives.
- Linux x86_64: standalone server archive (built on Ubuntu 22.04).
- Windows x86_64: GUI executable archive (SSH sessions only; no local server).

Extract the desktop archive and move `Scytty.app` into Applications. Its matching
server is bundled inside, so no separate server installation is needed locally.
The app is ad-hoc signed, **not notarized**; macOS Gatekeeper may block it until
explicitly allowed in System Settings.

On Windows, extract the desktop archive and run
`scytty.exe --ssh user@host [session]`. OpenSSH (`ssh.exe`) must be on PATH,
and `scytty-server` must be installed on the remote Unix host. To build locally,
use `cargo build --release -p scytty` with Rust, Zig and the MSVC build tools.

Publish a release with `git tag v0.1.0 && git push origin v0.1.0` (use a new tag
for each release).

## Configuration

The GUI evaluates `~/.config/scytty/config.lua` with bundled **LuaJIT** on startup
(or `$XDG_CONFIG_HOME/scytty/config.lua`). Return a table:

```lua
return {
  font_size = 16,
  background_opacity = 0.8,
  window_width = 960,
  window_height = 640,
}
```

See `config.example.lua`. Missing options use defaults; unknown options and
invalid values are errors. Config applies to terminal and tab-bar fonts.
Set `font_family = "Menlo"` (or another installed family name) to select a font.
The default `"monospace"` selects the system monospace family. The regular face
is used; unknown families produce a startup error. No bundled glyph fallback is
provided. Choose a monospace family for correct terminal alignment.
Restart the GUI to reload. Only load trusted config: this is executable Lua.

Customize the native window title with `format_window_title = function(ctx)
return ctx.session .. " - " .. ctx.title end`. It receives `session`, the active
tab's `title`, and `remote` (boolean), and must return a string.
Server settings and custom keybindings are not configurable yet.

Modules beside `config.lua` work with `require()`, including `module/init.lua`.
For example, `config.format_tab = require("format-tab-title")` loads
`~/.config/scytty/format-tab-title.lua`, which should return a callback. Settings
modules can return a function and apply it with `require("settings")(config)`.
No working-directory change or WezTerm API shim is required.

The bar is a single transparent monospace text row, with clickable tabs and `+`.
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
that space automatically. Set `icon_scale = 1.5` (default; range 1–3) to configure their
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
own tab's progress. It survives detach/reattach, including over SSH. Display is
opt-in, for example:

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

This changes tab metadata on the wire: update both GUI and server and restart
the daemon when safe (restarting ends existing sessions).

## Default shortcuts

- **Ctrl+Plus / Ctrl+Minus** (also Cmd on macOS): increase/decrease font size
- **Ctrl+0** (also Cmd+0 on macOS): reset to the configured font size
- **Drag**: select terminal text
- **Cmd+C / Cmd+V** on macOS, **Ctrl+Shift+C / Ctrl+Shift+V** elsewhere:
  copy selection / paste clipboard (including over SSH)
- **Cmd+T** (Ctrl+T elsewhere): new tab
- **Cmd+Left / Cmd+Right** (Ctrl+Left / Ctrl+Right elsewhere): previous/next tab, wrapping at the ends

Tab text and the `+` character also work with the mouse.

The GUI runs locally; the server supports macOS and Linux. The default shell
is `$SHELL`, falling back to `/bin/sh`. Windows is not supported.

## Remote sessions over SSH

Build and install the headless server on the Linux host (same source revision
as the client; Rust 1.96+ and Zig 0.17.x required for building):

```sh
cargo build --release -p scytty-server
install -m 755 target/release/scytty-server ~/.local/bin/scytty-server
```

Ensure `scytty-server` is on PATH for non-interactive SSH commands. Then locally:

```sh
cargo run -p scytty --release -- --ssh user@host work
```

One SSH connection carries the **entire session**, including every tab's
input, output, snapshots and control messages. The system `ssh` client handles
authentication, host keys and SSH config aliases (including ports/ProxyJump).
Use an SSH agent or authenticate from the launching terminal. No TCP listener
or exposed server port is needed. The remote server starts automatically.

Closing the window or losing SSH detaches without killing remote shells.
Run the same command to reattach; automatic reconnect is not implemented.
A fixed startup handshake checks protocol versions before any session frames
are decoded, locally and over SSH. Incompatible or pre-handshake peers produce
an explicit update/restart error rather than attempting to decode session data.
This is a compatibility check, not automatic version conversion. Bump
`PROTOCOL_VERSION` in `crates/common/src/protocol.rs` when message or Ghostty snapshot
formats change incompatibly. Restarting the daemon ends existing sessions.

Connection/startup and runtime errors appear in a selectable text panel with a
**Copy error** button, preserving the full error chain for diagnostics.
A full Linux runtime/real SSH deployment has not been tested here.

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
Use a Kitty-compatible sender with **direct/stream transfer**, also over SSH
(for example `kitten icat --transfer-mode=stream picture.png`).

Minimal smoke test (a red block):

```sh
printf '\033_Ga=T,q=2,f=24,s=1,v=1,i=1,c=10,r=5;/wAA\033\\'
printf '\033_Ga=d,d=A\033\\' # delete
```

Image storage is capped at 16 MiB per terminal screen. PNG decoding is bounded
to 16 MiB RGBA and 4096 pixels per dimension; GPU texture limits may be lower.
File, temporary-file and shared-memory transfer are disabled: programs must send
image bytes through the PTY. Sixel, iTerm2 images, animation playback and Kitty
Unicode-placeholder/relative placements are not rendered yet.
Rebuild both GUI and server; restart the daemon when safe to enable images in
new shells (restarting ends existing sessions).

## Included

- Interactive login shell, Ctrl shortcuts, navigation and F1–F12 keys
- ANSI colors, alternate screen, terminal replies and resize propagation
- Clipboard paste with Ghostty's bracketed-paste encoding
- Drop files into the window to paste shell-quoted paths (multiple files supported).
  In SSH sessions these are local paths, not file uploads.
- Mouse-wheel scrollback, configurable with `scrollback_mb` (default 16 MiB per tab)
- Multiple tabs, server-owned PTYs, live state/output synchronization
- Gzip-compressed binary protocol over a private local Unix socket
- Bounded output queues with backpressure for image/output bursts; stalled
  socket writes disconnect after their 5-second timeout. A stalled client may
  briefly pause other sessions while the server waits for queue capacity.

## Limits

One attached window per session; no splits yet. No mouse reporting,
full font styling, or complete extended keyboard/IME support.
Unicode text is passed through, but wide/combined glyph rendering needs
more work. Clipboard access requested by terminal programs is denied.
There is no session-list UI yet.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
