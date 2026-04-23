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

To enable the in-app `/agent` command with Groq, either add your API key to `.env.local`:

```powershell
GROQ_API_KEY=your-groq-api-key
```

or set it in the environment before launching:

```powershell
$env:GROQ_API_KEY="your-groq-api-key"
cargo run
```

## Build

```powershell
cargo build --release
```

## GitHub Actions artifacts

GitHub Actions builds downloadable release artifacts for macOS and Windows:

- `velocity-macos-intel`
- `velocity-macos-apple-silicon`
- `velocity-windows`

You can run the workflows from the Actions tab with `workflow_dispatch`, or let them run automatically on pushes to `main` and on pull requests.
