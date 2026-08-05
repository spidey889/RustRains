//! RustRains — Matrix-style falling code rain in the terminal.
//! Pure `std` only: no crates. ANSI escapes for color and cursor control.
//! Exit with Ctrl+C.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// ANSI helpers
// ---------------------------------------------------------------------------

const HIDE_CURSOR: &str = "\x1b[?25l";
const SHOW_CURSOR: &str = "\x1b[?25h";
const CLEAR: &str = "\x1b[2J";
const HOME: &str = "\x1b[H";
const RESET: &str = "\x1b[0m";
const BRIGHT_WHITE: &str = "\x1b[97m";
const BRIGHT_GREEN: &str = "\x1b[92m";
const GREEN: &str = "\x1b[32m";
const DIM_GREEN: &str = "\x1b[2;32m";

/// Glyphs used in the rain (ASCII + half-width katakana for Matrix flavour).
const GLYPHS: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E',
    'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T',
    'U', 'V', 'W', 'X', 'Y', 'Z', ':', '.', '"', '=', '*', '+', '-', '<', '>',
    'ｱ', 'ｲ', 'ｳ', 'ｴ', 'ｵ', 'ｶ', 'ｷ', 'ｸ', 'ｹ', 'ｺ', 'ｻ', 'ｼ', 'ｽ', 'ｾ', 'ｿ',
    'ﾀ', 'ﾁ', 'ﾂ', 'ﾃ', 'ﾄ', 'ﾅ', 'ﾆ', 'ﾇ', 'ﾈ', 'ﾉ', 'ﾊ', 'ﾋ', 'ﾌ', 'ﾍ', 'ﾎ',
    'ﾏ', 'ﾐ', 'ﾑ', 'ﾒ', 'ﾓ', 'ﾔ', 'ﾕ', 'ﾖ', 'ﾗ', 'ﾘ', 'ﾙ', 'ﾚ', 'ﾛ', 'ﾜ', 'ﾝ',
];

static RUNNING: AtomicBool = AtomicBool::new(true);

// ---------------------------------------------------------------------------
// Tiny xorshift PRNG (no `rand` crate)
// ---------------------------------------------------------------------------

struct Rng {
    state: u64,
}

impl Rng {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xDEAD_BEEF);
        let addr = &nanos as *const u64 as u64;
        Self {
            state: nanos ^ addr.rotate_left(17) | 1,
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn gen_usize(&mut self, max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        (self.next_u64() as usize) % max
    }

    fn gen_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
    }

    fn glyph(&mut self) -> char {
        GLYPHS[self.gen_usize(GLYPHS.len())]
    }
}

// ---------------------------------------------------------------------------
// Terminal size + platform setup
// ---------------------------------------------------------------------------

fn terminal_size() -> (usize, usize) {
    #[cfg(windows)]
    {
        if let Some(size) = windows_size() {
            return size;
        }
    }

    let cols = std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80usize)
        .max(10);
    let rows = std::env::var("LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24usize)
        .max(5);
    (cols, rows)
}

#[cfg(windows)]
fn windows_size() -> Option<(usize, usize)> {
    #[repr(C)]
    struct Coord {
        x: i16,
        y: i16,
    }
    #[repr(C)]
    struct SmallRect {
        left: i16,
        top: i16,
        right: i16,
        bottom: i16,
    }
    #[repr(C)]
    struct ConsoleScreenBufferInfo {
        size: Coord,
        cursor_position: Coord,
        attributes: u16,
        window: SmallRect,
        maximum_window_size: Coord,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(n_std_handle: i32) -> *mut core::ffi::c_void;
        fn GetConsoleScreenBufferInfo(
            handle: *mut core::ffi::c_void,
            info: *mut ConsoleScreenBufferInfo,
        ) -> i32;
    }

    const STD_OUTPUT_HANDLE: i32 = -11;
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() || handle == (-1isize as *mut _) {
            return None;
        }
        let mut info = std::mem::zeroed::<ConsoleScreenBufferInfo>();
        if GetConsoleScreenBufferInfo(handle, &mut info) == 0 {
            return None;
        }
        let cols = (info.window.right - info.window.left + 1) as usize;
        let rows = (info.window.bottom - info.window.top + 1) as usize;
        if cols < 10 || rows < 5 {
            return None;
        }
        Some((cols, rows))
    }
}

#[cfg(windows)]
fn enable_ansi() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(n_std_handle: i32) -> *mut core::ffi::c_void;
        fn GetConsoleMode(handle: *mut core::ffi::c_void, mode: *mut u32) -> i32;
        fn SetConsoleMode(handle: *mut core::ffi::c_void, mode: u32) -> i32;
    }

    const STD_OUTPUT_HANDLE: i32 = -11;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    const ENABLE_PROCESSED_OUTPUT: u32 = 0x0001;

    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() || handle == (-1isize as *mut _) {
            return;
        }
        let mut mode: u32 = 0;
        if GetConsoleMode(handle, &mut mode) != 0 {
            let _ = SetConsoleMode(
                handle,
                mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING | ENABLE_PROCESSED_OUTPUT,
            );
        }
    }
}

#[cfg(not(windows))]
fn enable_ansi() {}

fn restore_terminal() {
    let mut out = io::stdout();
    let _ = write!(out, "{SHOW_CURSOR}{RESET}{CLEAR}{HOME}");
    let _ = out.flush();
}

/// Restores the terminal on normal drop paths.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

#[cfg(windows)]
fn install_ctrl_c_handler() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }

    unsafe extern "system" fn handler(_ctrl_type: u32) -> i32 {
        RUNNING.store(false, Ordering::SeqCst);
        restore_terminal();
        // Return TRUE (1) so the process is not killed immediately — we exit
        // cleanly from the main loop.
        1
    }

    unsafe {
        let _ = SetConsoleCtrlHandler(Some(handler), 1);
    }
}

#[cfg(not(windows))]
fn install_ctrl_c_handler() {
    // Without crates there is no portable SIGINT hook in std. Ctrl+C will
    // terminate the process; most terminals reset the cursor afterwards.
    // The Drop guard still covers normal exits.
}

// ---------------------------------------------------------------------------
// Rain columns
// ---------------------------------------------------------------------------

struct Column {
    /// Head row (can be above the screen while spawning).
    y: f64,
    /// Rows advanced per frame.
    speed: f64,
    /// Length of the fading trail.
    length: usize,
    /// Glyphs currently in this column's trail (index 0 = head).
    trail: Vec<char>,
}

impl Column {
    fn new(rows: usize, rng: &mut Rng) -> Self {
        let length = 5 + rng.gen_usize(rows.max(10) / 2);
        let mut trail = Vec::with_capacity(length);
        for _ in 0..length {
            trail.push(rng.glyph());
        }
        Self {
            // Stagger starts so the screen doesn't fill at once.
            y: -(rng.gen_usize(rows * 2) as f64),
            speed: 0.15 + rng.gen_f64() * 0.85,
            length,
            trail,
        }
    }

    fn reset(&mut self, rows: usize, rng: &mut Rng) {
        self.length = 5 + rng.gen_usize(rows.max(10) / 2);
        self.trail.clear();
        for _ in 0..self.length {
            self.trail.push(rng.glyph());
        }
        self.y = -(rng.gen_usize(rows) as f64) - 1.0;
        self.speed = 0.15 + rng.gen_f64() * 0.85;
    }

    fn tick(&mut self, rows: usize, rng: &mut Rng) {
        self.y += self.speed;
        // Occasionally mutate a glyph mid-fall for that classic flicker.
        if rng.gen_usize(8) == 0 && !self.trail.is_empty() {
            let i = rng.gen_usize(self.trail.len());
            self.trail[i] = rng.glyph();
        }
        // Rotate a fresh glyph into the head.
        if rng.gen_usize(3) == 0 {
            self.trail.insert(0, rng.glyph());
            if self.trail.len() > self.length {
                self.trail.pop();
            }
        }
        if self.y - self.length as f64 > rows as f64 {
            self.reset(rows, rng);
        }
    }
}

// ---------------------------------------------------------------------------
// Frame buffer
// ---------------------------------------------------------------------------

struct Cell {
    ch: char,
    /// 0 = empty, 1 = dim, 2 = mid, 3 = bright, 4 = head
    style: u8,
}

fn paint_frame(cols: &mut [Column], width: usize, height: usize, rng: &mut Rng) -> String {
    let mut grid: Vec<Cell> = (0..width * height)
        .map(|_| Cell {
            ch: ' ',
            style: 0,
        })
        .collect();

    for (x, col) in cols.iter_mut().enumerate() {
        col.tick(height, rng);
        let head = col.y as isize;
        for (i, &ch) in col.trail.iter().enumerate() {
            let y = head - i as isize;
            if y < 0 || y >= height as isize {
                continue;
            }
            let style = if i == 0 {
                4
            } else if i == 1 {
                3
            } else if i < col.length / 3 {
                2
            } else {
                1
            };
            let idx = y as usize * width + x;
            // Prefer brighter cells when streams overlap.
            if style >= grid[idx].style {
                grid[idx] = Cell { ch, style };
            }
        }
    }

    let mut out = String::with_capacity(width * height * 8);
    out.push_str(HOME);

    let mut last_style = 255u8;
    for y in 0..height {
        for x in 0..width {
            let cell = &grid[y * width + x];
            if cell.style != last_style {
                match cell.style {
                    0 => out.push_str(RESET),
                    1 => out.push_str(DIM_GREEN),
                    2 => out.push_str(GREEN),
                    3 => out.push_str(BRIGHT_GREEN),
                    _ => out.push_str(BRIGHT_WHITE),
                }
                last_style = cell.style;
            }
            out.push(if cell.style == 0 { ' ' } else { cell.ch });
        }
        if y + 1 < height {
            out.push_str("\r\n");
        }
    }
    out.push_str(RESET);
    out
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn main() {
    enable_ansi();
    install_ctrl_c_handler();

    let (width, height) = terminal_size();
    let mut rng = Rng::new();
    let mut columns: Vec<Column> = (0..width).map(|_| Column::new(height, &mut rng)).collect();

    let _guard = TerminalGuard;
    let mut stdout = io::stdout();
    let _ = write!(stdout, "{HIDE_CURSOR}{CLEAR}{HOME}");
    let _ = stdout.flush();

    let frame_time = Duration::from_millis(40); // ~25 FPS
    let mut last_size = (width, height);

    while RUNNING.load(Ordering::SeqCst) {
        let frame_start = Instant::now();

        let (w, h) = terminal_size();
        if (w, h) != last_size {
            columns = (0..w).map(|_| Column::new(h, &mut rng)).collect();
            last_size = (w, h);
            let _ = write!(stdout, "{CLEAR}");
        }

        let frame = paint_frame(&mut columns, last_size.0, last_size.1, &mut rng);
        let _ = write!(stdout, "{frame}");
        let _ = stdout.flush();

        let elapsed = frame_start.elapsed();
        if elapsed < frame_time {
            thread::sleep(frame_time - elapsed);
        }
    }
}
