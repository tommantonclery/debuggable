# cargo-debuggable

Sets up GDB, LLDB and VS Code to show [`debuggable`](https://crates.io/crates/debuggable)
visualizers, and diagnoses them when they don't show.

```sh
cargo install cargo-debuggable

cargo debuggable setup --dry-run      # see what would change
cargo debuggable setup                # configure every debugger found
cargo debuggable doctor target/debug/my-app
```

`setup` configures:
- **GDB:** trusts the current project's `target/` directory. Run it once per project.
- **LLDB:** installs a loader and imports it from `~/.lldbinit`, for `lldb` and `rust-lldb`.
- **VS Code (CodeLLDB):** adds the loader to `lldb.launch.preRunCommands`, so every project
  works without `launch.json` changes.

It only edits clearly marked blocks, backs up each file before its first change, and leaves
hand-made configuration alone. `cargo debuggable setup --remove` restores everything exactly.

`doctor` checks your debuggers, your configuration and (given a path) a binary, and prints the
fix for each problem it finds. For a Linux binary it also checks that every described type is in
the debug info under the name debuggers will look for, and points out types they can't match,
such as types defined inside a function.

See the [`debuggable` README](https://github.com/tommantonclery/debuggable) for the full guide.
