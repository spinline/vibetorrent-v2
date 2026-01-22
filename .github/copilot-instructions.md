<!-- Use this file to provide workspace-specific custom instructions to Copilot. -->

## VibeTorrent Project

Modern rTorrent Web UI built with:

- **Backend**: Rust + Axum (async web framework)
- **Templates**: Askama (compile-time template engine)
- **Frontend**: HTMX + Hyperscript (no JavaScript frameworks)
- **Styling**: Tailwind CSS CLI (dark theme)
- **Connection**: SCGI protocol to rTorrent

### Project Structure

- `src/main.rs` - Application entry point and router setup
- `src/routes.rs` - HTTP route handlers
- `src/rtorrent.rs` - SCGI client for rTorrent communication
- `src/templates.rs` - Askama template definitions
- `src/state.rs` - Application state
- `src/error.rs` - Error handling
- `templates/` - HTML templates with HTMX
- `static/css/` - Tailwind CSS files

### Development Commands

```bash
# Run Tailwind CSS watch mode
npm run css

# Run the application
cargo run

# Build for production
npm run css:build
cargo build --release
```

### Environment Variables

- `RTORRENT_SCGI_SOCKET` - Path to rTorrent SCGI socket (default: `/tmp/rtorrent.sock`)
- `BIND_ADDRESS` - Server bind address (default: `0.0.0.0:3000`)
- `RUST_LOG` - Log level

### FOUC Prevention Rules

1. Critical CSS is inlined in the `<head>` of base.html
2. A preload overlay is shown until CSS and JS are fully loaded
3. Background colors are set immediately to prevent white flash
4. Content uses `visibility: hidden` until ready

### Askama Template Syntax

- Use `{% if %}...{% else %}...{% endif %}` (no `elif`)
- Use `{{ variable }}` for output
- Use `{% include "partial.html" %}` for partials
- Use `{% for item in items %}...{% endfor %}` for loops
