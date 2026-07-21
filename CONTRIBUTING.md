# Contributing to m68k

Thank you for your interest in contributing to the Motorola 68000 (m68k) assembler, disassembler, and floppy-image toolkit! We welcome bug reports, feature requests, documentation improvements, and code contributions.

Here are some guidelines to help you get started.

## How to Contribute

### 1. Reporting Bugs & Requesting Features
Before opening a new issue, please search the existing issues to see if it has already been reported or requested.

If you find a new bug or have a feature request:
1. Open an issue on GitHub.
2. Provide a clear description of the issue or feature.
3. For bugs, include a minimal reproducible example (e.g., the raw hex bytes of the instruction causing the issue and the expected vs. actual output).

### 2. Development Setup

To set up a local development environment:

1. **Clone the repository**:
   ```bash
   git clone https://github.com/Attackwave/m68k.git
   cd m68k
   ```

2. **Install Rust** (via [rustup](https://rustup.rs/)) if you don't already have it.

3. **Build the workspace**:
   ```bash
   cargo build --all
   ```

### 3. Running Tests

Please ensure all tests, clippy, and formatting checks pass before submitting a Pull Request:

```bash
cargo test --all
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

If you add new instruction definitions or parsing rules, you **must** add corresponding unit tests in the relevant crate (e.g. `crates/m68k-disasm/src/decoder.rs` for decoder changes, `crates/m68k-asm/src/enc_*.rs` for encoder changes).

### 4. Coding Guidelines

- **Docstrings**: Document all new public items with rustdoc comments in English.
- **English Comments**: Keep comments and variable names entirely in English.
- **`cargo fmt`/`cargo clippy`**: Code must be clean under `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings`.
- **Conventional Commits**: We encourage formatting your commit messages in the Conventional Commits style (e.g. `feat: ...`, `fix: ...`, `docs: ...`, `build: ...`).

### 5. Submitting a Pull Request (PR)

1. Fork the repository and create your branch from `main`.
   ```bash
   git checkout -b my-feature-branch
   ```
2. Commit your changes with clear, descriptive commit messages.
3. Push your branch to your fork.
4. Open a Pull Request against our `main` branch.
5. Provide a description of the changes in the PR template.
