# Clipboard Desktop

A high-performance cross-platform clipboard manager built with Tauri 2 and Rust,
featuring full-text search, OCR, and a modern Svelte 5 UI.

## Prerequisites

- [Node.js](https://nodejs.org/) >= 18
- [Rust](https://www.rust-lang.org/tools/install) stable toolchain
- Platform-specific Tauri 2 [system dependencies](https://v2.tauri.app/start/prerequisites/)

### Windows

```powershell
# Install Rust via rustup (includes Cargo)
winget install Rustlang.Rustup

# Install Node.js (LTS)
winget install OpenJS.NodeJS.LTS
```

### macOS

```sh
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Node.js via Homebrew
brew install node
```

### Linux (Ubuntu/Debian)

```sh
# Tauri 2 system dependencies
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev libappindicator3-dev \
  librsvg2-dev patchelf libssl-dev libgtk-3-dev \
  libjavascriptcoregtk-4.1-dev libsoup-3.0-dev

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Node.js
curl -fsSL https://deb.nodesource.com/setup_lts.x | sudo -E bash -
sudo apt install -y nodejs
```

## Quick Start

```sh
# Install frontend dependencies
npm install

# Start development server with hot reload
npm run tauri dev

# Or run frontend only (browser preview with demo data)
npm run dev
```

## Scripts

| Command                | Description                                    |
| ---------------------- | ---------------------------------------------- |
| `npm run dev`          | Start Vite dev server (frontend only)          |
| `npm run build`        | Build frontend for production                  |
| `npm run tauri dev`    | Start Tauri desktop app in dev mode            |
| `npm run tauri build`  | Build production Tauri desktop app             |
| `npm run check`        | TypeScript type checking                       |
| `npm run format`       | Format code with Prettier + Cargo fmt          |
| `npm run format:check` | Check formatting without modifying files       |
| `npm run test:rust`    | Run Rust unit tests                            |
| `npm run lint:rust`    | Run Clippy linter on Rust code                 |
| `npm run verify`       | Run all checks: format, typecheck, build, test |

## Project Structure

```
clipboard/
├── src/                        # Svelte 5 frontend (SPA mode)
│   ├── app.html                # HTML shell
│   ├── app.css                 # Global dark theme styles
│   ├── routes/                 # SvelteKit routes
│   │   ├── +page.svelte        # Main clipboard UI page
│   │   ├── +layout.svelte      # Root layout
│   │   └── +layout.ts          # SSR disabled for Tauri
│   └── lib/
│       ├── components/         # Reusable Svelte components
│       ├── services/           # Tauri invoke wrappers
│       ├── types/              # TypeScript type definitions
│       ├── utils/              # Utility functions
│       └── data/               # Demo data for browser preview
├── src-tauri/                  # Rust backend
│   ├── tauri.conf.json         # Tauri 2 configuration
│   ├── Cargo.toml              # Rust dependencies
│   ├── capabilities/           # Tauri 2 permissions
│   ├── icons/                  # App icons
│   └── src/
│       ├── main.rs             # Entry point
│       ├── lib.rs              # Command registration & setup
│       ├── config.rs           # App config store
│       ├── domain/             # Domain models
│       ├── keyboard/           # Shortcut parsing & matching
│       ├── ocr/                # OCR engine interface
│       ├── platform/           # Platform capability abstraction
│       ├── search/             # Tantivy full-text search
│       └── storage/            # SQLite database & repositories
├── docs/                       # Design & architecture docs
├── static/                     # Static assets
├── TODO.md                     # Project roadmap
└── package.json                # NPM configuration
```

## Technology Stack

| Layer    | Technology                                              |
| -------- | ------------------------------------------------------- |
| Desktop  | [Tauri 2](https://v2.tauri.app/)                        |
| Backend  | Rust, rusqlite (SQLite), Tantivy (full-text search)     |
| Frontend | Svelte 5 + TypeScript, SvelteKit (SPA mode), Vite       |
| Search   | Tantivy with custom N-gram tokenizer (Chinese-friendly) |
| Storage  | SQLite with migrations and repository pattern           |

## Recommended IDE Extensions

- [Svelte for VS Code](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode)
- [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode)
- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## License

MIT
