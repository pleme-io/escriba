//! ANSI rendering of the start screen — the `--render=text` face.
//!
//! All layout comes from `escriba_ui::splash`; this only turns roles into
//! 24-bit SGR sequences. The GPU face consumes the SAME
//! [`Splash::screen_chunks`](escriba_ui::splash::Splash::screen_chunks)
//! stream and turns roles into glyphon `Attrs` instead, which is what keeps
//! the three faces from laying the screen out three ways.

use escriba_ui::chrome::ChromePalette;
use escriba_ui::splash::{Splash, SplashRole};

/// Reset every attribute — emitted once at the end so a caller's terminal
/// is handed back clean.
const SGR_RESET: &str = "\x1b[0m";

/// Render `splash` on a `width × height` character canvas as ANSI text.
///
/// Returns an empty string when the canvas is too small to hold even the
/// compact wordmark — a caller then falls back to its ordinary frame rather
/// than printing a mangled one.
#[must_use]
pub fn render_splash_ansi(
    splash: &Splash,
    chrome: &ChromePalette,
    width: u16,
    height: u16,
) -> String {
    let chunks = splash.screen_chunks(width, height);
    if chunks.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(chunks.len() * 24);
    // `None` = nothing emitted yet, so the first colored chunk always
    // writes its sequence.
    let mut current: Option<SplashRole> = None;
    for chunk in &chunks {
        // Whitespace carries no color, and re-emitting SGR around every
        // pad run would triple the output for no visible difference.
        if chunk.text.trim().is_empty() {
            out.push_str(&chunk.text);
            continue;
        }
        if current != Some(chunk.role) {
            push_fg(&mut out, chunk.role.color(chrome));
            current = Some(chunk.role);
        }
        out.push_str(&chunk.text);
    }
    out.push_str(SGR_RESET);
    out
}

/// `ESC[38;2;R;G;Bm` — a 24-bit foreground. Built with `push_str`/`push`
/// rather than `format!`, per the fleet's typed-emission rule.
fn push_fg(out: &mut String, c: ishou_tokens::Rgb) {
    out.push_str("\x1b[38;2;");
    push_u8(out, c.r);
    out.push(';');
    push_u8(out, c.g);
    out.push(';');
    push_u8(out, c.b);
    out.push('m');
}

fn push_u8(out: &mut String, mut n: u8) {
    if n == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 3];
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10);
        n /= 10;
    }
    out.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("0"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use escriba_core::Action;
    use escriba_ui::splash::SplashEntry;

    fn sample() -> Splash {
        Splash {
            art: vec!["ESCRIBA".into()],
            tagline: "a modal editor".into(),
            entries: vec![SplashEntry {
                key: 'q',
                label: "quit".into(),
                action: Action::Quit,
            }],
            facts: vec!["v0.1.0".into()],
        }
    }

    #[test]
    fn the_screen_survives_the_trip_through_ansi() {
        let out = render_splash_ansi(&sample(), &ChromePalette::prescribed(), 80, 24);
        assert!(out.contains("ESCRIBA"), "{out:?}");
        assert!(out.contains("a modal editor"));
        assert!(out.contains("quit"));
        assert!(out.contains("v0.1.0"));
    }

    #[test]
    fn color_is_emitted_and_always_reset() {
        let out = render_splash_ansi(&sample(), &ChromePalette::prescribed(), 80, 24);
        assert!(out.contains("\x1b[38;2;"), "no 24-bit color emitted");
        assert!(
            out.ends_with(SGR_RESET),
            "a face must hand the terminal back clean",
        );
    }

    #[test]
    fn a_canvas_with_no_room_renders_nothing_at_all() {
        // Not a partial screen — the caller falls back to its own frame.
        assert!(render_splash_ansi(&sample(), &ChromePalette::prescribed(), 80, 1).is_empty());
    }

    #[test]
    fn push_u8_covers_the_whole_byte_range() {
        for n in [0u8, 7, 42, 100, 255] {
            let mut s = String::new();
            push_u8(&mut s, n);
            assert_eq!(s, n.to_string());
        }
    }
}
