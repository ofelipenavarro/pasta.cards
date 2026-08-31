//! Sidebar data panel: card-index freshness + the in-app updater.
//!
//! Port of `desktop/ui/js/sidebar.js` atop the shell's data channel. The
//! JS polled `/api/data/update/status` once a second from the browser;
//! here the worker does the polling (`UpdateStatus` command) and the
//! answers arrive as `UpdateStatusLoaded` events, which is also what
//! drives the progress bar — no extra timers.
//!
//! Layout (footer band, below the offline badge):
//! - "Baixar base de dados agora" when the index doesn't exist, else
//!   "Atualizar base de dados" plus the card-count line;
//! - task text + percentage while running, error text when it fails;
//! - the button re-enables on settle, and a completed update refreshes
//!   the info line (like the JS's `renderSidebarDataPanel()` on done).

use engine::compositor::{Compositor, SceneNode};
use engine::theme::Theme;
use engine::ui::widgets::{
    Button, ProgressBar, Rect, WidgetEvent,
};
use spellbook_core::client::{Command, Event};
use spellbook_core::ops::data::{DataInfo, UpdateStatus};

use super::text;
use crate::view::ScreenCtx;

const _PANEL_H: f32 = 96.0;

pub struct DataPanel {
    info: Option<DataInfo>,
    status: Option<UpdateStatus>,
    /// Poll the worker again (set while running; cleared on answer).
    poll_elapsed: f32,
    update_btn: Button,
    error: Option<String>,
}

impl DataPanel {
    pub fn new() -> Self {
        Self {
            info: None,
            status: None,
            poll_elapsed: 0.0,
            update_btn: Button::new("Atualizar base de dados"),
            error: None,
        }
    }

    /// First load + every sidebar tick re-asks when a poll is due (1s, like
    /// the JS's `setTimeout(tick, 1000)`).
    pub fn boot(&mut self, ctx: &ScreenCtx) {
        ctx.send(Command::DataInfo);
        if self.needs_polling() {
            ctx.send(Command::UpdateStatus);
        }
    }

    fn needs_polling(&self) -> bool {
        self.update_btn.disabled
    }

    pub fn on_event(&mut self, event: &Event, ctx: &mut ScreenCtx) -> bool {
        let mut changed = false;
        match event {
            Event::DataInfoLoaded(info) => {
                self.info = Some(info.as_ref().clone());
                self.update_btn.label = (if info.exists {
                    "Atualizar base de dados"
                } else {
                    "Baixar base de dados agora"
                })
                .into();
                changed = true;
            }
            Event::UpdateStatusLoaded(status) => {
                self.poll_elapsed = 0.0;
                match status.state.as_str() {
                    "running" => {
                        changed = self.info.is_none();
                    }
                    "error" => {
                        self.error = status.error.clone();
                        self.update_btn.disabled = false;
                        changed = true;
                    }
                    "done" => {
                        self.error = None;
                        self.update_btn.disabled = false;
                        // A finished update may have built the index for the
                        // first time or refreshed it; the info line follows.
                        ctx.send(Command::DataInfo);
                        changed = true;
                    }
                    _ => {
                        self.update_btn.disabled = false;
                        changed = true;
                    }
                }
                self.status = Some(status.clone());
            }
            _ => {}
        }
        changed
    }

    pub fn tick(&mut self, dt: f32, ctx: &ScreenCtx) -> bool {
        if !self.update_btn.disabled {
            return false;
        }
        // 1-second poll cadence while an update runs.
        self.poll_elapsed += dt;
        if self.poll_elapsed >= 1.0 {
            self.poll_elapsed = 0.0;
            ctx.send(Command::UpdateStatus);
        }
        self.update_btn.disabled
    }

    pub fn handle_event(&mut self, event: &WidgetEvent, panel: Rect, ctx: &ScreenCtx) -> bool {
        if let WidgetEvent::MouseDown { x, y } = *event {
            if self.update_btn_rect(panel).contains(x, y) && !self.update_btn.disabled {
                self.update_btn.disabled = true;
                self.update_btn.label = "Atualizando…".into();
                self.error = None;
                ctx.send(Command::UpdateStart);
                ctx.send(Command::UpdateStatus);
                return true;
            }
        }
        false
    }

    fn update_btn_rect(&self, panel: Rect) -> Rect {
        let (w, h) = self.update_btn.preferred_size();
        Rect::new(panel.x + 24.0, panel.y + 10.0, w.min(panel.w - 48.0), h)
    }

    fn status_rect(&self, panel: Rect) -> Rect {
        Rect::new(
            panel.x + 24.0,
            panel.y + 58.0,
            panel.w - 48.0,
            30.0,
        )
    }

    pub fn render(&mut self, c: &mut Compositor, panel: Rect, theme: &Theme) {
        // Soft divider above (the footer band's top edge).
        c.push(SceneNode::Rect {
            x: panel.x,
            y: panel.y,
            w: panel.w,
            h: 1.0,
            color: theme.glass.edge_soft.0,
        });

        self.update_btn.render(c, self.update_btn_rect(panel), theme);

        // Info line: what the index is and how fresh.
        let status_rect = self.status_rect(panel);
        if let Some(status) = &self.status
            && status.state == "running"
        {
            let pct = (status.percent / 100.0).clamp(0.0, 1.0);
            ProgressBar::new(pct as f32).render(c, status_rect, theme);
            let label = format!(
                "{} · {}%",
                status.task.as_deref().unwrap_or("Atualizando…"),
                status.percent.round() as i64
            );
            text(
                c,
                &label,
                10.0,
                500,
                panel.x + 24.0,
                status_rect.y - 14.0,
                theme.colors.text_dim.0,
            );
            return;
        }
        if let Some(error) = &self.error {
            text(
                c,
                &format!("Falhou: {error}"),
                10.0,
                500,
                panel.x + 24.0,
                status_rect.y,
                theme.colors.danger.0,
            );
            return;
        }
        let line = match &self.info {
            Some(info) if info.exists => {
                let fresh = info
                    .built_at
                    .map(|t| format!(" · atualizado em {}", format_built_at(t)))
                    .unwrap_or_default();
                format!(
                    "{} cartas + EDHREC cache{fresh}. Só preço ao vivo precisa de rede.",
                    info.cards
                )
            }
            Some(_) => "Base de cartas ainda não configurada.".into(),
            None => "Carregando info da base…".into(),
        };
        let dim = theme.glass.text_placeholder.0;
        c.push(SceneNode::Text {
            key: engine::compositor::TextNodeKey::from_style(
                &line,
                &engine::theme::TypographyScale::hoff().caption_sm(),
                Some(status_rect.w),
            ),
            x: panel.x + 24.0,
            y: status_rect.y,
            color: dim,
        });
    }
}

/// Unix seconds -> "dd/mm/aaaa hh:mm", `formatBuiltAt`'s pt-BR shape.
fn format_built_at(unix_seconds: f64) -> String {
    // Civil-from-days (Howard Hinnant's algorithm), no chrono dependency.
    let secs = unix_seconds.floor() as i64;
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let sod = secs.rem_euclid(86_400);
    let (hh, mm) = (sod / 3600, (sod % 3600) / 60);
    format!("{d:02}/{m:02}/{y} {hh:02}:{mm:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> (ScreenCtx<'static>, std::sync::mpsc::Receiver<Command>) {
        use crate::view::ScreenCtx;
        let (tx, rx) = std::sync::mpsc::channel();
        let tx_box = Box::leak(Box::new(tx));
        let actions = Box::leak(Box::new(Vec::new()));
        (ScreenCtx { tx: tx_box, actions }, rx)
    }

    fn running_status() -> UpdateStatus {
        UpdateStatus {
            state: "running".into(),
            task: Some("Baixando bulk data".into()),
            percent: 42.0,
            error: None,
            result: None,
        }
    }

    #[test]
    fn start_update_disables_and_polls() {
        let mut panel = DataPanel::new();
        let (mut ctx, rx) = test_ctx();
        let panel_rect = Rect::new(0.0, 800.0, 248.0, _PANEL_H);
        let rect = panel.update_btn_rect(panel_rect);

        let changed = panel.handle_event(
            &WidgetEvent::MouseDown { x: rect.x + 5.0, y: rect.y + 5.0 },
            panel_rect,
            &mut ctx,
        );
        assert!(changed);
        assert!(panel.update_btn.disabled);
        // UpdateStart, then UpdateStatus.
        let first = rx.try_recv().unwrap();
        let second = rx.try_recv().unwrap();
        assert!(matches!(first, Command::UpdateStart));
        assert!(matches!(second, Command::UpdateStatus));

        // Running status keeps the button down.
        panel.on_event(&Event::UpdateStatusLoaded(running_status()), &mut ctx);
        assert!(panel.update_btn.disabled);
    }

    #[test]
    fn done_reenables_and_refreshes_info() {
        let mut panel = DataPanel::new();
        let (mut ctx, rx) = test_ctx();
        panel.update_btn.disabled = true;
        let done = UpdateStatus {
            state: "done".into(),
            task: None,
            percent: 100.0,
            error: None,
            result: None,
        };
        panel.on_event(&Event::UpdateStatusLoaded(done), &mut ctx);
        assert!(!panel.update_btn.disabled);
        let cmd = rx.try_recv().unwrap();
        assert!(matches!(cmd, Command::DataInfo));
    }

    #[test]
    fn error_shows_text_and_reenables() {
        let mut panel = DataPanel::new();
        let (mut ctx, _rx) = test_ctx();
        panel.update_btn.disabled = true;
        let failed = UpdateStatus {
            state: "error".into(),
            task: None,
            percent: 30.0,
            error: Some("rede caiu".into()),
            result: None,
        };
        panel.on_event(&Event::UpdateStatusLoaded(failed), &mut ctx);
        assert!(!panel.update_btn.disabled);
        assert_eq!(panel.error.as_deref(), Some("rede caiu"));
    }

    #[test]
    fn built_at_formats_brazilian() {
        // 2026-08-30 16:04:11 UTC = 1785477600... use a known pair: epoch 0.
        // 1970-01-01 00:00 -> "01/01/1970 00:00"
        assert_eq!(format_built_at(0.0), "01/01/1970 00:00");
        // 2000-03-01 12:00 UTC = 951912000
        assert_eq!(format_built_at(951_912_000.0), "01/03/2000 12:00");
    }
}