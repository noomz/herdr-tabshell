# herdr-tabshell

A herdr plugin that gives each herdr tab its own persistent tmux shell, shown in a per-workspace float. See `README.md` for behavior and `GLOSSARY.md` for vocabulary (tab shell, shell float, binding).

It needs the patched herdr on `noomz/herdr` branch `local`: floats, hide on a click outside, focus from a key press, and per-workspace floats. Stock herdr has none of these.

## Layout

- `herdr-plugin.toml`: the manifest. It has the `toggle` action, the `shell` float pane, and the `tab.focused` hook. All of them run `target/release/herdr-tabshell`.
- `src/command.rs`: the `toggle`, `shell` and `tab-focused` subcommands, and the per-workspace float state.
- `src/tmux.rs`: `ensure_window` (tab-ID bindings in `@herdr_tab`), the exit hook, and the tmux tests.
- `src/herdr.rs`: a raw socket client for herdr. `src/lock.rs`: a `flock` held across each command. `src/config.rs`: parses `tabshell.conf`.
- `tmux.conf`: the shipped server config. It runs before the user's `tmux.conf`.

Dependencies are limited to `serde_json` and `libc`: a raw socket and no async runtime. The `tab.focused` hook must exit within 10 ms when it has nothing to do.

## Commands

```bash
cargo build --release   # the manifest runs the release binary, so rebuild before testing in herdr
cargo test
cargo test <name>       # a single test
cargo clippy --release --all-targets
```

The tests start tmux servers on throwaway `-S` sockets under `/tmp`. Never point a test at the `herdr-tabshell` socket or any real tmux server. The hook tests need `fish` on `PATH`.
