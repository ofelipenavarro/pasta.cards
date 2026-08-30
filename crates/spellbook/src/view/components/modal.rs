//! Modal scaffold for the spellbook screens.
//!
//! `ModalFrame` is the glass dialog shell (backdrop + panel + title bar)
//! used by [`AddCardModal`](super::add_card::AddCardModal) and
//! [`CardModal`](super::card_modal::CardModal). It is intentionally lower
//! level than [`engine::ui::widgets::Modal`]: the content, buttons and
//! answers live in the owning modal.
//!
//! `ConfirmDialog` is a simple yes/no wrapper on top of the engine's
//! ready-made [`Modal`] widget.

use engine::compositor::{Compositor, LayerId, SceneNode, TextNodeKey};
use engine::theme::{Intent, Theme, TypographyScale};
use engine::ui::widgets::{EventResult, Modal as EngineModal, Rect, WidgetEvent, glass_pill, menu_shadow, rounded_rect};

/// Padding between the modal panel edge and its content.
pub const PAD: f32 = 24.0;
const TITLE_H: f32 = 56.0;
const RADIUS: f32 = 24.0;
const CLOSE_SIZE: f32 = 32.0;

/// Glass dialog shell: backdrop, panel, title and close button.
#[derive(Clone, Debug, Default)]
pub struct ModalFrame {
    hover_close: bool,
}

impl ModalFrame {
    pub fn new() -> Self {
        Self::default()
    }

    /// Center a `width x height` panel inside `window`, clamped to fit.
    pub fn rect(&self, window: Rect, width: f32, height: f32) -> Rect {
        let w = width.min(window.w - 32.0);
        let h = height.min(window.h - 32.0);
        Rect::new(
            window.x + (window.w - w) / 2.0,
            window.y + (window.h - h) / 2.0,
            w,
            h,
        )
    }

    /// Content area inside the panel, below the title bar.
    pub fn content_rect(&self, panel: Rect) -> Rect {
        Rect::new(
            panel.x + PAD,
            panel.y + TITLE_H,
            panel.w - PAD * 2.0,
            panel.h - TITLE_H - PAD,
        )
    }

    fn close_rect(&self, panel: Rect) -> Rect {
        Rect::new(
            panel.x + panel.w - PAD - CLOSE_SIZE,
            panel.y + (TITLE_H - CLOSE_SIZE) / 2.0,
            CLOSE_SIZE,
            CLOSE_SIZE,
        )
    }

    /// Pointer handling. Returns `(close_requested, event_result)`.
    pub fn handle_event(&mut self, event: &WidgetEvent, panel: Rect) -> (bool, EventResult) {
        match *event {
            WidgetEvent::MouseMove { x, y } => {
                let hovered = self.close_rect(panel).contains(x, y);
                if hovered != self.hover_close {
                    self.hover_close = hovered;
                    return (false, EventResult::changed());
                }
                (false, EventResult::IGNORED)
            }
            WidgetEvent::MouseDown { x, y } => {
                if self.close_rect(panel).contains(x, y) {
                    return (true, EventResult::clicked());
                }
                if !panel.contains(x, y) {
                    // Backdrop click closes.
                    return (true, EventResult::clicked());
                }
                (false, EventResult::IGNORED)
            }
            _ => (false, EventResult::IGNORED),
        }
    }

    pub fn render(
        &self,
        c: &mut Compositor,
        _layer: LayerId,
        window: Rect,
        panel: Rect,
        title: &str,
        theme: &Theme,
    ) {
        // Backdrop scrim.
        c.push(SceneNode::RoundedRect {
            x: window.x,
            y: window.y,
            w: window.w,
            h: window.h,
            color: [0.08, 0.08, 0.10, 0.85],
            corner_radius: 0.0,
            border_width: 0.0,
            border_color: [0.0; 4],
        });

        // Glass panel.
        for node in glass_pill(
            panel,
            RADIUS,
            theme.glass.edge_soft.0,
            1.5,
            theme.glass.surface.0,
        ) {
            c.push(node);
        }
        c.push(menu_shadow(panel, RADIUS));

        // Title.
        let style = TypographyScale::hoff().title();
        c.push(SceneNode::Text {
            key: TextNodeKey::from_style(title, &style, None),
            x: panel.x + PAD,
            y: panel.y + (TITLE_H - style.line_height) / 2.0,
            color: theme.colors.text.0,
        });

        // Close button.
        let close = self.close_rect(panel);
        if self.hover_close {
            c.push(rounded_rect(
                close.x,
                close.y,
                close.w,
                close.h,
                theme.radius.md,
                theme.glass.surface_hover.0,
            ));
        }
        if let Some(node) = engine::ui::icons::icon_at(
            "x",
            16.0,
            theme.colors.text_dim.0,
            close.x + (close.w - 16.0) / 2.0,
            close.y + (close.h - 16.0) / 2.0,
        ) {
            c.push(node);
        }

        // Divider under title.
        c.push(SceneNode::RoundedRect {
            x: panel.x + PAD,
            y: panel.y + TITLE_H - 1.0,
            w: panel.w - PAD * 2.0,
            h: 1.0,
            color: theme.glass.edge_soft.0,
            corner_radius: 0.5,
            border_width: 0.0,
            border_color: [0.0; 4],
        });
    }
}

/// Simple wrapper around engine::ui::widgets::Modal for yes/no confirms.
pub struct Modal {
    engine_modal: EngineModal,
    open: bool,
}

impl Modal {
    pub fn new(
        title: impl Into<String>,
        body: impl Into<String>,
        confirm_label: impl Into<String>,
        cancel_label: impl Into<String>,
    ) -> Self {
        Self {
            engine_modal: EngineModal::new(title, body, confirm_label, cancel_label),
            open: false,
        }
    }

    pub fn intent(mut self, intent: Intent) -> Self {
        self.engine_modal = self.engine_modal.intent(intent);
        self
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn handle_overlay_event(&mut self, event: &WidgetEvent, vw: f32, vh: f32) -> bool {
        if !self.open {
            return false;
        }
        let (action, result) = self.engine_modal.handle_event(event, vw, vh);
        match action {
            engine::ui::widgets::ModalAction::Confirm
            | engine::ui::widgets::ModalAction::Cancel => {
                self.open = false;
                true
            }
            engine::ui::widgets::ModalAction::None => result.handled,
        }
    }

    pub fn render(&self, c: &mut Compositor, layer: LayerId, theme: &Theme, vw: f32, vh: f32) {
        if !self.open {
            return;
        }
        self.engine_modal.render(c, layer, theme, vw, vh);
    }
}

/// A confirm dialog (yes/no) built on Modal.
pub struct ConfirmDialog {
    modal: Modal,
    result: Option<bool>,
}

impl ConfirmDialog {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        _confirm_label: impl Into<String>,
        _cancel_label: impl Into<String>,
        danger: bool,
    ) -> Self {
        let modal = Modal::new(title, message, "Confirmar", "Cancelar")
            .intent(if danger { Intent::Destructive } else { Intent::Constructive });
        Self {
            modal,
            result: None,
        }
    }

    pub fn open(&mut self) {
        self.modal.open();
        self.result = None;
    }

    pub fn close(&mut self) {
        self.modal.close();
    }

    pub fn is_open(&self) -> bool {
        self.modal.is_open()
    }

    pub fn take_result(&mut self) -> Option<bool> {
        self.result.take()
    }

    pub fn handle_overlay_event(&mut self, event: &WidgetEvent, vw: f32, vh: f32) -> bool {
        if !self.modal.is_open() {
            return false;
        }
        let consumed = self.modal.handle_overlay_event(event, vw, vh);
        if !self.modal.is_open() && self.result.is_none() {
            self.result = Some(false);
        }
        consumed
    }

    pub fn render(&self, c: &mut Compositor, layer: LayerId, theme: &Theme, vw: f32, vh: f32) {
        self.modal.render(c, layer, theme, vw, vh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modal_opens_and_closes() {
        let mut modal = Modal::new("Test", "Body", "OK", "Cancel");
        assert!(!modal.is_open());
        modal.open();
        assert!(modal.is_open());
        modal.close();
        assert!(!modal.is_open());
    }
}
