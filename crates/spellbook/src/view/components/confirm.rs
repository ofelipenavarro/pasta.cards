//! Yes/no confirmation dialog. Port of `desktop/ui/js/ui/confirm.js` atop
//! `engine::ui::widgets::Modal`, which already encodes the same contract:
//! backdrop click cancels, a blocking dialog swallows everything else.
//!
//! The JS answers a promise; here the owner opens a [`Confirm`], forwards
//! events, and reads [`ConfirmAction`] off each call. Escape cancels and
//! Enter confirms, the keyboard behaviour the native `window.confirm` had
//! (and wry's delegate lacked - the reason this dialog exists at all).

use engine::compositor::{Compositor, LayerId};
use engine::theme::{Intent, Theme};
use engine::ui::widgets::{EventResult, Modal, ModalAction, Rect, WidgetEvent};

use super::super::EditKey;

/// What the user decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmAction {
    /// Still open.
    Pending,
    Confirmed,
    Cancelled,
}

pub struct Confirm {
    modal: Modal,
}

impl Confirm {
    /// Defaults match the JS: "Confirmar"/"Cancelar", `danger` recolors the
    /// confirm button.
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            modal: Modal::new(title, message, "Confirmar", "Cancelar"),
        }
    }

    pub fn labels(mut self, confirm: impl Into<String>, cancel: impl Into<String>) -> Self {
        self.modal = Modal::new(self.modal.title.clone(), self.modal.body.clone(), confirm, cancel)
            .intent(self.modal.intent);
        self
    }

    /// Red confirm button, like `danger = true` in the JS.
    pub fn danger(mut self) -> Self {
        self.modal = self.modal.intent(Intent::Destructive);
        self
    }

    /// Enter confirms, anything else is the buttons' or the backdrop's.
    pub fn handle_event(
        &mut self,
        event: &WidgetEvent,
        window: Rect,
    ) -> (ConfirmAction, EventResult) {
        let (action, result) = self.modal.handle_event(event, window.w, window.h);
        let action = match action {
            ModalAction::Confirm => ConfirmAction::Confirmed,
            ModalAction::Cancel => ConfirmAction::Cancelled,
            ModalAction::None => ConfirmAction::Pending,
        };
        (action, result)
    }

    pub fn handle_edit_key(&mut self, key: EditKey) -> ConfirmAction {
        match key {
            EditKey::Enter => ConfirmAction::Confirmed,
            _ => ConfirmAction::Pending,
        }
    }

    /// Escape cancels (and stays the shell's close signal: the owner drops
    /// the dialog when this returns true).
    pub fn handle_escape(&mut self) -> ConfirmAction {
        ConfirmAction::Cancelled
    }

    pub fn render(&mut self, c: &mut Compositor, layer: LayerId, window: Rect, theme: &Theme) {
        self.modal.render(c, layer, theme, window.w, window.h);
    }
}

use engine::compositor::SceneNode;
use engine::ui::widgets::{glass_pill, rounded_rect};
use super::super::text;

/// The two-button confirm dialog, matching `confirm.js` treatment.
pub fn render_confirm_dialog(
    c: &mut Compositor,
    layer: LayerId,
    window: Rect,
    theme: &Theme,
    title: &str,
    message: &str,
    buttons: (&str, bool),
) {
    // Dim backdrop.
    c.push_to_layer(
        layer,
        SceneNode::Rect {
            x: window.x,
            y: window.y,
            w: window.w,
            h: window.h,
            color: [0.0, 0.0, 0.0, 0.42],
        },
    );
    let pw = 380.0f32.min(window.w - 32.0);
    let ph = 190.0f32.min(window.h - 32.0);
    let panel_rect = Rect::new(
        window.x + (window.w - pw) / 2.0,
        window.y + (window.h - ph) / 2.0,
        pw,
        ph,
    );
    for node in glass_pill(
        panel_rect,
        theme.radius.lg,
        theme.glass.edge.0,
        1.5,
        theme.glass.popover.0,
    ) {
        c.push_to_layer(layer, node);
    }
    text(c, title, 15.0, 600, panel_rect.x + 20.0, panel_rect.y + 18.0, theme.colors.text.0);
    text(
        c,
        message,
        12.0,
        400,
        panel_rect.x + 20.0,
        panel_rect.y + 48.0,
        theme.colors.text_dim.0,
    );

    let (confirm_label, danger) = buttons;
    let cx = window.x + window.w / 2.0;
    let by = window.y + window.h / 2.0 + 40.0;
    let yes = Rect::new(cx - 230.0, by, 140.0, 42.0);
    let no = Rect::new(cx + 90.0, by, 120.0, 42.0);
    c.push_to_layer(
        layer,
        rounded_rect(
            yes.x,
            yes.y,
            yes.w,
            yes.h,
            theme.radius.md,
            if danger {
                theme.colors.danger.0
            } else {
                theme.colors.accent.0
            },
        ),
    );
    text(c, confirm_label, 12.0, 600, yes.x + 20.0, yes.y + 14.0, [0.9, 0.9, 0.9, 1.0]);
    c.push_to_layer(
        layer,
        rounded_rect(no.x, no.y, no.w, no.h, theme.radius.md, theme.glass.surface_active.0),
    );
    text(c, "Cancelar", 12.0, 500, no.x + 18.0, no.y + 14.0, theme.colors.text.0);
}
