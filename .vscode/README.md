## VS Code Setup

- Install the recommended extensions when VS Code prompts:
  - `rust-lang.rust-analyzer`
  - `vadimcn.vscode-lldb`
- Use `Terminal > Run Build Task` for `cargo build`.
- Use `Terminal > Run Test Task` for `cargo test`.
- Use the Testing view for Rust tests discovered by `rust-analyzer`.
- Use the `Debug AWEBPinator` launch configuration to run the GTK app under LLDB.
- `rust-analyzer` is configured to run `clippy --all-targets --all-features` for editor diagnostics.
