# Velocity

A minimal Rust command-line application.

## Prerequisites

Install Rust with `rustup`:

```powershell
winget install Rustlang.Rustup
```

Then restart your terminal.

## Run

```powershell
cargo run
```

## Build

```powershell
cargo build --release
```

## macOS artifacts

GitHub Actions builds downloadable macOS release artifacts for both Intel and Apple Silicon:

- `velocity-macos-intel`
- `velocity-macos-apple-silicon`

You can run the workflow from the Actions tab with `workflow_dispatch`, or let it run automatically on pushes to `main` and on pull requests.
