# VibeTorrent

Modern rTorrent Web UI built with Rust, Askama, HTMX, Hyperscript, and Tailwind CSS.

## Features

- 🚀 Full Server-Side Rendering (SSR) with Rust/Axum
- 🎨 Modern dark UI matching rTorrent design
- ⚡ Real-time updates with HTMX
- 🔧 Client-side interactions with Hyperscript
- 💅 Tailwind CSS with strict FOUC prevention
- 🔌 SCGI connection to rTorrent

## Requirements

- Rust 1.70+
- Node.js 18+ (for Tailwind CSS)
- rTorrent with SCGI socket enabled

## Setup

### 1. Configure rTorrent SCGI Socket

Add to your `.rtorrent.rc`:

```
scgi_local = /tmp/rtorrent.sock
```

Or via network:

```
scgi_port = 127.0.0.1:5000
```

### 2. Install Dependencies

```bash
# Install Node.js dependencies for Tailwind
npm install

# Build CSS
npm run css:build
```

### 3. Configure Environment

Create a `.env` file or set environment variables:

```env
RTORRENT_SCGI_SOCKET=/tmp/rtorrent.sock
BIND_ADDRESS=0.0.0.0:3000
RUST_LOG=vibetorrent=debug
```

### 4. Build and Run

```bash
# Development
cargo run

# Production
cargo build --release
./target/release/vibetorrent
```

### 5. Development with CSS Watch

In one terminal:
```bash
npm run css
```

In another terminal:
```bash
cargo run
```

## Project Structure

```
vibetorrent/
├── src/
│   ├── main.rs         # Application entry point
│   ├── error.rs        # Error handling
│   ├── routes.rs       # HTTP route handlers
│   ├── rtorrent.rs     # rTorrent SCGI client
│   ├── state.rs        # Application state
│   └── templates.rs    # Askama template definitions
├── templates/
│   ├── base.html       # Base layout with FOUC prevention
│   ├── index.html      # Main page
│   └── partials/       # HTMX partial templates
│       ├── torrent_list.html
│       ├── torrent_row.html
│       ├── stats.html
│       └── add_torrent_modal.html
├── static/
│   └── css/
│       ├── input.css   # Tailwind input
│       └── output.css  # Compiled CSS
├── Cargo.toml
├── tailwind.config.js
└── package.json
```

## FOUC Prevention

This project implements strict FOUC (Flash of Unstyled Content) prevention:

1. **Critical CSS Inline**: Essential styles are inlined in `<head>`
2. **Loading State**: Content is hidden until CSS loads
3. **Preload Overlay**: Smooth transition from loading to ready state
4. **Background Colors**: Set immediately to prevent white flash

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | Main page |
| GET | `/torrents` | Torrent list partial |
| GET | `/torrents/filter/{filter}` | Filtered torrent list |
| POST | `/torrent/{hash}/pause` | Pause torrent |
| POST | `/torrent/{hash}/resume` | Resume torrent |
| POST | `/torrent/{hash}/remove` | Remove torrent |
| POST | `/torrent/{hash}/toggle-star` | Toggle star |
| GET | `/add-torrent` | Add torrent modal |
| POST | `/add-torrent` | Add torrent (URL/file) |
| GET | `/stats` | Stats partial |

## License

MIT
