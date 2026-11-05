# herdr-tabshell

A herdr plugin that gives each herdr tab its own floating tmux shell on `prefix+f`.

Status: planning. This repo holds docs, with no code yet. Do not start a build ticket until the user says to.

The plan lives in `.wayfinder/map.md` and its tickets. `.wayfinder/README.md` explains how to find the next ticket and record a resolution. Vocabulary is in `GLOSSARY.md`.

## Rules

- Do not kill the user's herdr sessions or kitty windows. A new herdr binary needs a live handoff; tell the user what to run.
- Do not touch the live `herdr-floax` tmux socket when testing. Use a throwaway socket.
- Do not use `mv` on a worktree. Use `git -C ../herdr worktree move`, or `worktree remove` to delete one.
- Leave the unrelated dirty files in the `../herdr` main checkout alone. Do herdr work in the worktree.
- Local git only for this repo. Add no remote until the user asks.
- Use `/opt/homebrew/bin/rg`. In this shell `rg` can resolve to BSD grep.

## Commands

**herdr patches.** Read `../herdr/AGENTS.md` for that project's rules. From this directory:

```bash
# Create the worktree (once)
git -C ../herdr worktree add ../herdr-worktrees/float-hide-focus -b feat/float-hide-focus local
cd ../herdr-worktrees/float-hide-focus

# Fast signal, run first
cargo check --all-targets

# herdr's own runners; run `just check` before committing
just test               # cargo nextest + maintenance script tests
just check              # formatting check + the above
```

If `just` is not installed, run the justfile's nextest command directly (about 4000 tests).

Never run plain `cargo test` in herdr. It runs in a single process and gives false failures and SIGPIPE (rc=101). `terminal_attach_detach_deletes_forwarded_kitty_images` is known flaky; rerun it alone before judging.

Build the herdr release with `cargo build --release --locked` from a clean tree. If the `local-apps-update` skill is available, follow its herdr install steps.

**This plugin**, once the scaffold ticket lands. It is a Rust crate, and as in `../herdr-pip` the manifest points at the release binary, so rebuild before testing in herdr:

```bash
cargo build --release
cargo test
cargo test <name>       # single test
```
