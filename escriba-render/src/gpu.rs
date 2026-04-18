//! GPU renderer — implements [`madori::RenderCallback`] backed by garasu's
//! glyphon-wrapped text renderer. Each frame:
//!
//!   1. Locks the shared `EditorState`.
//!   2. Collects visible buffer lines into a single string.
//!   3. Builds a glyphon `Buffer` (re-created each frame — phase 1.B; phase 2
//!      will diff + reuse).
//!   4. Prepares + renders through `madori::RenderContext::text`.
//!
//! Nord colors come from `irodori`. Text is rendered in Nord6 (snow storm
//! bright foreground) over a Nord0 (polar night) background. The status line
//! is rendered inverted in Nord8 (frost accent).

use std::sync::{Arc, Mutex};

use escriba_core::Mode;
use escriba_runtime::EditorState;
use glyphon::{Attrs, Buffer, Color as GlyphColor, Family, Metrics, Shaping, TextArea, TextBounds};
use irodori::NORD;
use madori::{RenderCallback, RenderContext};

/// Shared handle to the editor state — both the GPU renderer (reads) and
/// the madori `on_event` callback (writes) hold one.
pub type SharedState = Arc<Mutex<EditorState>>;

/// The GPU render callback.
///
/// Holds a shared reference to the editor state. `render()` reads it under
/// lock, computes a frame, releases the lock before touching the GPU to
/// minimise contention with the event loop.
pub struct GpuRenderer {
    state: SharedState,
    font_size: f32,
    line_height: f32,
    /// Cached font metrics — rebuilt if font_size changes.
    metrics: Metrics,
}

impl GpuRenderer {
    #[must_use]
    pub fn new(state: SharedState) -> Self {
        let font_size = 14.0;
        let line_height = 20.0;
        Self {
            state,
            font_size,
            line_height,
            metrics: Metrics::new(font_size, line_height),
        }
    }

    #[must_use]
    pub fn with_font_size(mut self, font_size: f32, line_height: f32) -> Self {
        self.font_size = font_size;
        self.line_height = line_height;
        self.metrics = Metrics::new(font_size, line_height);
        self
    }
}

impl RenderCallback for GpuRenderer {
    fn render(&mut self, ctx: &mut RenderContext<'_>) {
        // ── 1. Read state under lock, build display text. ──────────────
        let (text, mode_str, cursor_line, cursor_col) = {
            let s = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(buf) = s.buffers.get(s.active) else {
                return clear_frame(ctx);
            };
            let win = s.layout.active_window().cloned();
            let top_line = win.as_ref().map_or(0, |w| w.viewport.top_line);
            let visible_lines = win
                .as_ref()
                .map_or(40, |w| w.viewport.visible_lines.max(20));
            let mut out = String::new();
            for row in 0..visible_lines {
                let ln = top_line + row;
                if ln >= buf.line_count() {
                    break;
                }
                if let Some(line) = buf.line(ln) {
                    out.push_str(line.trim_end_matches('\n').trim_end_matches('\r'));
                    out.push('\n');
                }
            }
            (out, s.modal.mode.as_str(), s.cursor.line, s.cursor.column)
        };

        // ── 2. Layout glyphon buffer. ─────────────────────────────────
        let fg = nord_color(NORD.snow_storm[2]); // Nord6 — brightest foreground
        let mut buffer = Buffer::new(&mut ctx.text.font_system, self.metrics);
        let width = ctx.width as f32;
        let height = ctx.height as f32 - self.line_height; // reserve bottom row for status
        buffer.set_size(&mut ctx.text.font_system, Some(width), Some(height));
        buffer.set_text(
            &mut ctx.text.font_system,
            &text,
            &Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut ctx.text.font_system, false);

        // Status line — rendered as its own glyphon buffer.
        let status = format!(
            " {}  {}:{}  escriba v{} ",
            mode_str,
            cursor_line + 1,
            cursor_col + 1,
            env!("CARGO_PKG_VERSION")
        );
        let mut status_buf = Buffer::new(&mut ctx.text.font_system, self.metrics);
        status_buf.set_size(
            &mut ctx.text.font_system,
            Some(width),
            Some(self.line_height * 2.0),
        );
        status_buf.set_text(
            &mut ctx.text.font_system,
            &status,
            &Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
        );
        status_buf.shape_until_scroll(&mut ctx.text.font_system, false);

        let status_color = nord_color(NORD.frost[1]); // Nord8

        let text_areas = [
            TextArea {
                buffer: &buffer,
                left: 8.0,
                top: 8.0,
                scale: 1.0,
                bounds: TextBounds {
                    left: 0,
                    top: 0,
                    right: ctx.width as i32,
                    bottom: (height as i32).max(0),
                },
                default_color: fg,
                custom_glyphs: &[],
            },
            TextArea {
                buffer: &status_buf,
                left: 8.0,
                top: (ctx.height as f32 - self.line_height - 4.0).max(0.0),
                scale: 1.0,
                bounds: TextBounds {
                    left: 0,
                    top: (ctx.height as i32 - self.line_height as i32 - 4).max(0),
                    right: ctx.width as i32,
                    bottom: ctx.height as i32,
                },
                default_color: status_color,
                custom_glyphs: &[],
            },
        ];

        if let Err(e) = ctx.text.prepare(
            &ctx.gpu.device,
            &ctx.gpu.queue,
            ctx.width,
            ctx.height,
            text_areas,
        ) {
            tracing::warn!(error = %e, "glyphon prepare failed");
            return clear_frame(ctx);
        }

        // ── 3. Encode frame. ───────────────────────────────────────────
        let mut encoder = ctx
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("escriba frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("escriba main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: ctx.surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(nord_bg()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if let Err(e) = ctx.text.render(&mut pass) {
                tracing::warn!(error = %e, "glyphon render failed");
            }
        }
        ctx.gpu.queue.submit(std::iter::once(encoder.finish()));
    }

    fn resize(&mut self, width: u32, height: u32) {
        if let Ok(mut s) = self.state.lock() {
            for w in &mut s.layout.windows {
                w.rect.width = width;
                w.rect.height = height;
                // Rough visible-line count from height / line_height.
                let lh = self.line_height.max(1.0);
                w.viewport.visible_lines = ((height as f32 / lh).max(1.0) as u32).saturating_sub(1);
            }
        }
    }
}

/// Utility — clear the frame to Nord background. Used on error paths.
fn clear_frame(ctx: &mut RenderContext<'_>) {
    let mut encoder = ctx
        .gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("escriba clear"),
        });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("escriba clear pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(nord_bg()),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    ctx.gpu.queue.submit(std::iter::once(encoder.finish()));
}

/// Nord polar-night background (Nord0) as `wgpu::Color`. Values are linear
/// RGB in 0..=1.
fn nord_bg() -> wgpu::Color {
    let c = NORD.polar_night[0];
    wgpu::Color {
        r: f64::from(c.r) / 255.0,
        g: f64::from(c.g) / 255.0,
        b: f64::from(c.b) / 255.0,
        a: 1.0,
    }
}

/// Nord color → glyphon `Color` (premultiplied sRGB u8 RGBA).
fn nord_color(c: irodori::Color) -> GlyphColor {
    GlyphColor::rgba(c.r, c.g, c.b, 0xFF)
}

/// Mode indicator color — used by higher-layer rendering paths that want a
/// glance-readable color. Insert = Frost, Normal = Snow Storm, Visual = Aurora.
#[must_use]
pub fn mode_color(mode: Mode) -> irodori::Color {
    match mode {
        Mode::Insert | Mode::Command => NORD.frost[1],
        Mode::Visual | Mode::VisualLine => NORD.aurora[4],
        Mode::Normal => NORD.snow_storm[2],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use escriba_buffer::BufferSet;

    #[test]
    fn nord_bg_is_polar_night() {
        let bg = nord_bg();
        // Nord0 = #2E3440 — approximately (0.18, 0.20, 0.25, 1.0).
        assert!((bg.r - 0.180).abs() < 0.02);
        assert!((bg.g - 0.204).abs() < 0.02);
        assert!((bg.b - 0.251).abs() < 0.02);
        assert_eq!(bg.a, 1.0);
    }

    #[test]
    fn renderer_construction_is_cheap() {
        let mut bufs = BufferSet::new();
        let id = bufs.scratch("hello\n");
        let state = Arc::new(Mutex::new(EditorState::new_with_buffer(bufs, id)));
        let _r = GpuRenderer::new(state);
    }

    #[test]
    fn mode_colors_differ_by_mode() {
        let n = mode_color(Mode::Normal);
        let i = mode_color(Mode::Insert);
        let v = mode_color(Mode::Visual);
        assert_ne!((n.r, n.g, n.b), (i.r, i.g, i.b));
        assert_ne!((n.r, n.g, n.b), (v.r, v.g, v.b));
    }
}
