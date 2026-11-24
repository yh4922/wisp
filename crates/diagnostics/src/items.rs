use std::time::Duration;

use editor::{Editor, MultiBufferOffset};
use gpui::{
    App, Context, Entity, EventEmitter, IntoElement, ParentElement, Render, Styled, Subscription,
    Task, WeakEntity, Window,
};
use language::Diagnostic;
use project::project_settings::{GoToDiagnosticSeverityFilter, ProjectSettings};
use settings::Settings;
use ui::{Button, Color, CountBadge, IconName, Tooltip, h_flex, prelude::*};
use util::ResultExt;
use workspace::{HideStatusItem, StatusItemView, ToolbarItemEvent, Workspace, item::ItemHandle};

use crate::{Deploy, IncludeWarnings, ProjectDiagnosticsEditor};

/// Displays diagnostic counts in the activity bar, or the diagnostic under the
/// cursor in the status bar.
pub struct DiagnosticIndicator {
    summary: project::DiagnosticSummary,
    workspace: WeakEntity<Workspace>,
    current_diagnostic: Option<Diagnostic>,
    active_editor: Option<WeakEntity<Editor>>,
    _observe_active_editor: Option<Subscription>,
    activity_bar: bool,

    diagnostics_update: Task<()>,
    diagnostic_summary_update: Task<()>,
}

impl Render for DiagnosticIndicator {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let indicator = h_flex().gap_2().min_w_0().overflow_x_hidden();
        if !ProjectSettings::get_global(cx).diagnostics.button {
            return indicator.hidden();
        }

        if self.activity_bar {
            return indicator
                .w_full()
                .child(self.render_activity_bar_button(cx));
        }

        // The error/warning counts live in the activity bar; the status bar
        // instance only shows the diagnostic under the cursor.
        let status = if let Some(diagnostic) = &self.current_diagnostic {
            let message = diagnostic
                .message
                .as_str()
                .split_once('\n')
                .map_or(diagnostic.message.as_str(), |(first, _)| first);
            let diagnostics_already_active = self.any_active_diagnostics(cx);
            let tooltip = if !diagnostics_already_active {
                i18n::t("Expand Diagnostics")
            } else {
                i18n::t("Next Diagnostic")
            };
            Some(
                Button::new("diagnostic_message", SharedString::new(message))
                    .label_size(LabelSize::Small)
                    .truncate(true)
                    .tab_index(0isize)
                    .tooltip(move |_window, cx| {
                        Tooltip::for_action(
                            tooltip,
                            &editor::actions::GoToDiagnostic::default(),
                            cx,
                        )
                    })
                    .on_click(
                        cx.listener(|this, _, window, cx| this.go_to_next_diagnostic(window, cx)),
                    ),
            )
        } else {
            None
        };

        indicator.children(status)
    }
}

impl DiagnosticIndicator {
    fn render_activity_bar_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (error_count, warning_count) = (self.summary.error_count, self.summary.warning_count);
        let diagnostics_label = match (error_count, warning_count) {
            (0, 0) => i18n::t("Project diagnostics: no problems").to_string(),
            (errors, warnings) => {
                let mut parts = Vec::new();
                if errors > 0 {
                    parts.push(if errors == 1 {
                        i18n::t("1 error").to_string()
                    } else {
                        i18n::tf!("{count} errors", count = errors)
                    });
                }
                if warnings > 0 {
                    parts.push(if warnings == 1 {
                        i18n::t("1 warning").to_string()
                    } else {
                        i18n::tf!("{count} warnings", count = warnings)
                    });
                }
                i18n::tf!("Project diagnostics: {parts}", parts = parts.join(", "))
            }
        };
        let icon_color = if error_count > 0 {
            Color::Error
        } else if warning_count > 0 {
            Color::Warning
        } else {
            Color::Muted
        };
        let tooltip_meta = SharedString::from(diagnostics_label.clone());

        let button =
            workspace::activity_bar::activity_bar_button("diagnostic-indicator", IconName::Warning)
                .icon_color(icon_color)
                .tab_index(0isize)
                .aria_label(diagnostics_label)
                .tooltip(move |_window, cx| {
                    Tooltip::with_meta(
                        i18n::t("Project Diagnostics"),
                        Some(&Deploy),
                        tooltip_meta.clone(),
                        cx,
                    )
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    if let Some(workspace) = this.workspace.upgrade() {
                        if this.summary.error_count == 0 && this.summary.warning_count > 0 {
                            cx.update_default_global(|show_warnings: &mut IncludeWarnings, _| {
                                show_warnings.0 = true
                            });
                        }
                        workspace.update(cx, |workspace, cx| {
                            ProjectDiagnosticsEditor::deploy(
                                workspace,
                                &Default::default(),
                                window,
                                cx,
                            )
                        })
                    }
                }));

        let total = error_count + warning_count;
        workspace::activity_bar::activity_bar_entry(
            div()
                .relative()
                .child(button)
                .when(total > 0, |this| this.child(CountBadge::new(total))),
            false,
            cx,
        )
    }

    /// Creates the compact variant shown in the activity bar, which only
    /// displays the error/warning counts.
    pub fn new_for_activity_bar(workspace: &Workspace, cx: &mut Context<Self>) -> Self {
        Self {
            activity_bar: true,
            ..Self::new(workspace, cx)
        }
    }

    pub fn new(workspace: &Workspace, cx: &mut Context<Self>) -> Self {
        let project = workspace.project();
        cx.subscribe(project, |this, project, event, cx| match event {
            project::Event::DiskBasedDiagnosticsStarted { .. } => {
                cx.notify();
            }

            project::Event::DiskBasedDiagnosticsFinished { .. }
            | project::Event::LanguageServerRemoved(_) => {
                this.summary = project.read(cx).diagnostic_summary(false, cx);
                cx.notify();
            }

            project::Event::DiagnosticsUpdated { .. } => {
                this.diagnostic_summary_update = cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(30))
                        .await;
                    this.update(cx, |this, cx| {
                        this.summary = project.read(cx).diagnostic_summary(false, cx);
                        cx.notify();
                    })
                    .log_err();
                });
            }

            _ => {}
        })
        .detach();

        Self {
            summary: project.read(cx).diagnostic_summary(false, cx),
            active_editor: None,
            workspace: workspace.weak_handle(),
            current_diagnostic: None,
            _observe_active_editor: None,
            activity_bar: false,
            diagnostics_update: Task::ready(()),
            diagnostic_summary_update: Task::ready(()),
        }
    }

    fn any_active_diagnostics(&self, cx: &mut Context<Self>) -> bool {
        if let Some(editor) = self.active_editor.as_ref().and_then(|e| e.upgrade()) {
            editor.read(cx).any_active_diagnostics()
        } else {
            false
        }
    }

    fn go_to_next_diagnostic(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor.as_ref().and_then(|e| e.upgrade()) {
            editor.update(cx, |editor, cx| {
                editor.go_to_diagnostic_at_cursor(
                    editor::Direction::Next,
                    GoToDiagnosticSeverityFilter::default(),
                    window,
                    cx,
                );
            })
        }
    }

    fn update(&mut self, editor: Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        let (buffer, cursor_position) = editor.update(cx, |editor, cx| {
            let buffer = editor.buffer().read(cx).snapshot(cx);
            let cursor_position = editor
                .selections
                .newest::<MultiBufferOffset>(&editor.display_snapshot(cx))
                .head();
            (buffer, cursor_position)
        });
        let new_diagnostic = buffer
            .diagnostics_in_range::<MultiBufferOffset>(cursor_position..cursor_position)
            .filter(|entry| !entry.range.is_empty())
            .min_by_key(|entry| {
                (
                    entry.diagnostic.severity,
                    entry.range.end - entry.range.start,
                )
            })
            .map(|entry| entry.diagnostic);
        if new_diagnostic != self.current_diagnostic.as_ref() {
            let new_diagnostic = new_diagnostic.cloned();
            self.diagnostics_update =
                cx.spawn_in(window, async move |diagnostics_indicator, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                    diagnostics_indicator
                        .update(cx, |diagnostics_indicator, cx| {
                            diagnostics_indicator.current_diagnostic = new_diagnostic;
                            cx.notify();
                        })
                        .ok();
                });
        }
    }
}

impl EventEmitter<ToolbarItemEvent> for DiagnosticIndicator {}

impl StatusItemView for DiagnosticIndicator {
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn ItemHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = active_pane_item.and_then(|item| item.downcast::<Editor>()) {
            self.active_editor = Some(editor.downgrade());
            self._observe_active_editor = Some(cx.observe_in(&editor, window, Self::update));
            self.update(editor, window, cx);
        } else {
            self.active_editor = None;
            self.current_diagnostic = None;
            self._observe_active_editor = None;
        }
        cx.notify();
    }

    fn hide_setting(&self, _: &App) -> Option<HideStatusItem> {
        Some(HideStatusItem::new(|settings| {
            settings.diagnostics.get_or_insert_default().button = Some(false);
        }))
    }
}
