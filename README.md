# RustRains

A Matrix-style falling code rain animation rendered entirely in the terminal — written in **pure Rust with zero external dependencies**.

## What It Is

RustRains is a single-binary terminal application that renders the iconic green cascading glyphs from _The Matrix_ in real time. Every component — random number generation, terminal size detection, ANSI escape rendering, signal handling — is implemented from scratch using only Rust's `std` library.

## Technical Overview

### Zero-Dependency Design

The `Cargo.toml` has an empty `[dependencies]` block. This means:

- **No `rand` crate** — randomness comes from a hand-rolled xorshift64 PRNG seeded with `SystemTime` nanoseconds XORed against a stack-pointer address for entropy.
- **No `crossterm` / `termion`** — all terminal control uses raw ANSI escape sequences (`\x1b[...`).
- **No `ctrlc` crate** — Ctrl+C handling is done via direct FFI to `kernel32!SetConsoleCtrlHandler` on Windows, with a no-op fallback on Unix.
- **No `terminal_size` crate** — console dimensions are read through FFI calls to `kernel32!GetConsoleScreenBufferInfo` on Windows, with `$COLUMNS`/`$LINES` env-var fallback on other platforms.

### Architecture

```
main()
  ├── enable_ansi()            // Windows: SetConsoleMode to enable VT processing
  ├── install_ctrl_c_handler() // Platform-specific Ctrl+C → AtomicBool flag
  ├── terminal_size()          // FFI (Windows) or env vars (Unix)
  │
  └── render loop @ ~25 FPS
        ├── terminal_size()        // Re-query each frame for live resize
        ├── Column::tick()         // Advance each rain column by its speed
        ├── paint_frame()          // Rasterize all columns into a cell grid
        │     ├── build grid[width × height] of Cell { char, style }
        │     ├── resolve overlaps (brighter style wins)
        │     └── serialize to ANSI-escaped string with diff-style tracking
        └── write + flush to stdout
```

### Rendering Pipeline

1. **Column simulation** — Each terminal column holds a `Column` struct tracking its vertical position (`f64` for sub-cell smoothness), fall speed, trail length, and a `Vec<char>` of active glyphs. Columns are staggered at spawn so the screen fills gradually.

2. **Glyph set** — The character pool mixes ASCII alphanumerics, symbols, and half-width Katakana (`ｱ`–`ﾝ`) to get that authentic Matrix aesthetic.

3. **Cell grid** — Every frame, a flat `Vec<Cell>` of size `width × height` is constructed. Each cell stores a character and a 5-level brightness style (`0`=empty, `1`=dim, `2`=mid, `3`=bright, `4`=head). When multiple streams overlap, the brighter style wins.

4. **ANSI serialization** — The grid is serialized into a single `String` with cursor-home (`\x1b[H`), inlined ANSI color codes, and `\r\n` line breaks. A `last_style` tracker avoids re-emitting redundant escape sequences to minimize bytes written per frame.

5. **Frame pacing** — A `Duration`-based frame timer targets ~25 FPS (`40ms/frame`). Elapsed render time is subtracted from the sleep interval to keep cadence steady.

### Platform Support

| Feature | Windows | Unix / macOS |
|---|---|---|
| Terminal size | `GetConsoleScreenBufferInfo` FFI | `$COLUMNS` / `$LINES` env vars |
| ANSI colors | `SetConsoleMode` VT processing | Native support |
| Ctrl+C handler | `SetConsoleCtrlHandler` FFI | Drop guard (process exit) |
| Signal safety | `AtomicBool` + `SeqCst` ordering | Same |

All Windows FFI is done inline via `#[link(name = "kernel32")]` blocks with `#[repr(C)]` struct definitions — no `winapi` or `windows-sys` crate needed.

### PRNG

The random number generator is a **xorshift64** implementation:

```rust
fn next_u64(&mut self) -> u64 {
    let mut x = self.state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    self.state = x;
    x
}
```

Seeded from `SystemTime::now().as_nanos() ^ (&nanos as *const u64 as u64).rotate_left(17)` — mixing wall-clock time with a stack address for non-determinism across runs.

### Terminal Cleanup

A `TerminalGuard` struct implements `Drop` to restore cursor visibility and clear the screen on any exit path (normal return, panic, or Ctrl+C). On Windows, the Ctrl+C handler also calls `restore_terminal()` directly before the main loop exits.

## Build & Run

```bash
# Clone
git clone https://github.com/spidey889/RustRains.git
cd RustRains

# Build (release for best performance)
cargo build --release

# Run
cargo run --release
```

**Requirements:** Rust Edition 2024 (rustc 1.85+). No other tooling needed.

Exit with **Ctrl+C**.

## Project Structure

```
RustRains/
├── Cargo.toml          # Manifest — zero dependencies
├── src/
│   └── main.rs         # Entire application (~400 lines)
├── .gitignore
└── README.md
```

## License

This project is open source. See the repository for license details.
