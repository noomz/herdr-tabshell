# herdr-tabshell

A [herdr](https://github.com/herdrdev/herdr) plugin that gives each herdr tab its own persistent shell. Press a key to show it in a floating window, and press it again to hide it.

- Each tab gets its own shell. It is a tmux window, so it keeps running while hidden, after the tab closes, and across a herdr live handoff.
- Each workspace has one float. It shows the shell of the current tab and appears only on its own workspace.
- The float takes keyboard focus when it opens. A click outside the float hides it.
- Move the float by dragging its title. Resize it by dragging its right or bottom border. It keeps its size and position until it closes.

> [!IMPORTANT]
> **This plugin needs a patched herdr.** Stock herdr (0.9.5) has no floating plugin panes. This plugin uses floats plus three patches: hide on a click outside, focus from a key press, and one float per workspace. They live on the `local` branch of [noomz/herdr](https://github.com/noomz/herdr/tree/local). On stock herdr, `herdr plugin install` fails, because stock herdr does not know the manifest's `placement = "float"`. `min_herdr_version` cannot express this requirement.

## Requirements

- herdr built from [`noomz/herdr@local`](https://github.com/noomz/herdr/tree/local)
- tmux (tested with 3.6a)
- Rust and `cargo`, to build the plugin
- macOS or Linux

## Install

1. Build and install the patched herdr:

   ```bash
   git clone -b local https://github.com/noomz/herdr
   cd herdr
   cargo build --release --locked
   cp target/release/herdr ~/.local/bin/herdr   # or wherever your herdr lives
   ```

   A server that is already running keeps the old binary. Run `herdr server live-handoff` to move your sessions to the new one.

2. Install the plugin. The install builds it with `cargo build --release --locked`:

   ```bash
   herdr plugin install noomz/herdr-tabshell
   ```

   To work on the plugin from a checkout instead, run `cargo build --release`, then `herdr plugin link .`. `link` does not build anything.

3. Bind a key in `~/.config/herdr/config.toml`:

   ```toml
   [[keys.command]]
   key = "prefix+f"
   type = "plugin_action"
   command = "herdr-tabshell.toggle"
   description = "tab shell"
   ```

   Then run `herdr server reload-config`.

## Use

| Action | Result |
|---|---|
| `prefix+f` | Show this workspace's float with the current tab's shell, or hide it |
| Click `x` in the float's title bar | Hide that float. The shell keeps running, and the next `prefix+f` shows it again at the same size and position. Needs a herdr with float title-bar buttons; an older herdr shows no button. |
| Click outside the float | Hide it, and the click goes to whatever you clicked |
| Switch tab with the keyboard while the float shows | The float follows the new tab and keeps focus |
| Click a tab in the tab bar | Counts as a click outside, so the float hides |
| Switch workspace | That workspace's float shows, or none |
| `exit` in the shown shell | The float closes; the next press makes a fresh shell |
| `C-a d` | Detach tmux, which closes the float |

**tmux prefix.** Inside the float, the tmux prefix is `C-a`, not `C-b`, because herdr's default prefix `C-b` reaches herdr first. So `C-a d` detaches and `C-a n` goes to the next window. If your herdr prefix is `C-a`, set another tmux prefix in the plugin's `tmux.conf` (see below). In herdr, pressing the prefix twice sends it to the pane, so `C-b C-b` also works.

## Configure

The config files live in the plugin config directory. Print its path with `herdr plugin config-dir herdr-tabshell`. It is usually `~/.config/herdr/plugins/config/herdr-tabshell/`.

**`tabshell.conf`** is read on every key press, so changes apply on the next press:

```ini
tab_windows = true   # false: one shared shell per workspace instead of one per tab
width = 80%          # percent of the screen, or a number of cells
height = 80%
```

**`tmux.conf`** is optional. The plugin runs its own tmux server (`tmux -L herdr-tabshell`), and it does not read `~/.tmux.conf`. The server loads the plugin's [`tmux.conf`](tmux.conf) and then this file, so anything you set here wins. For example, to hide the session name, which here is a herdr workspace ID:

```tmux
set -g status-left ""
set -g prefix C-s          # pick another tmux prefix
bind-key C-s send-prefix
```

To share settings with your main tmux, put the shared part in a separate file and `source-file` it from both. Leave out tpm plugins, tmux-continuum and tmux-resurrect: they would restore or save the tab shells. Also leave out `extended-keys` and `extkeys`: herdr's float sends shifted keys in a form that tmux then passes to the shell as raw text.

After editing, reload the running server:

```bash
tmux -L herdr-tabshell source-file "$(herdr plugin config-dir herdr-tabshell)/tmux.conf"
```

**Drop shadow.** The patched herdr can draw a drop shadow under floats. It is a herdr setting, not a plugin setting, and it is off by default. Add it to `~/.config/herdr/config.toml`:

```toml
[ui.float_shadow]
enabled = true
color = "#1d2021"   # hex, a color name, rgb(r,g,b), or "none" to only dim the text below
width = 2           # columns right of the float, 0 to 4
height = 1          # rows below the float, 0 to 4
```

Then run `herdr server reload-config`. A terminal cell is about twice as tall as it is wide, so `width = 2` and `height = 1` look even. `float_shadow = true` under `[ui]` gives a black shadow one cell wide. A click on the shadow counts as a click outside, so the float hides.

## Known limits

- **Two clients on one workspace share its float.** If two terminals attach to the same workspace, both see the same float.
- **Two herdr servers with the same workspace ID share a tmux session.**
- **Images:** `tdf` and other kitty-graphics viewers cannot draw through tmux.
- **Unfocused float:** a press on a float that shows but has no focus hides it. It does not focus it.
- **Leftover shells:** shells whose tab closed stay in tmux, because they may hold work. Reach them with tmux window keys, or kill them with `tmux -L herdr-tabshell kill-window`.

## Uninstall

```bash
herdr plugin uninstall herdr-tabshell   # or `herdr plugin unlink herdr-tabshell` for a linked checkout
```

Remove the `prefix+f` block from `config.toml` and run `herdr server reload-config`. The tab shells keep running. To end them, run `tmux -L herdr-tabshell kill-server`.

## License

[Apache-2.0](LICENSE)
