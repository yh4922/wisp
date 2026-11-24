use gpui::{AnyView, Context, Div, IntoElement, ParentElement, Render, Styled, Window};
use ui::{ButtonSize, IconButton, prelude::*};

/// Width of the VS Code style activity bar on the far left of the workspace.
pub const ACTIVITY_BAR_WIDTH: Pixels = px(48.);

/// Vertical strip of large view toggles (left dock panels, search, diagnostics)
/// rendered between the title bar and the status bar.
#[derive(Default)]
pub struct ActivityBar {
    items: Vec<AnyView>,
}

impl ActivityBar {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add_item(&mut self, item: impl Into<AnyView>, cx: &mut Context<Self>) {
        self.items.push(item.into());
        cx.notify();
    }
}

impl Render for ActivityBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        v_flex()
            .id("activity-bar")
            .flex_none()
            .w(ACTIVITY_BAR_WIDTH)
            .h_full()
            .py_1p5()
            .gap_2()
            .overflow_hidden()
            .bg(colors.status_bar_background)
            .border_t_1()
            .border_b_1()
            .border_r_1()
            .border_color(colors.border)
            .children(self.items.iter().cloned())
    }
}

/// Builds an icon button sized for the activity bar.
pub fn activity_bar_button(id: impl Into<ElementId>, icon: IconName) -> IconButton {
    IconButton::new(id, icon)
        .size(ButtonSize::Large)
        .width(rems_from_px(40_f32))
        .icon_size(IconSize::Custom(rems_from_px(24_f32)))
}

/// Wraps an activity bar button in a full-width row so the active item can show
/// a VS Code style accent strip flush against the window's left edge.
pub fn activity_bar_entry(button: impl IntoElement, active: bool, cx: &App) -> Div {
    let accent = cx.theme().colors().text_accent;
    h_flex()
        .relative()
        .w_full()
        .justify_center()
        .when(active, |this| {
            this.child(
                div()
                    .absolute()
                    .left_0()
                    .top_1()
                    .bottom_1()
                    .w(px(2.))
                    .bg(accent),
            )
        })
        .child(button)
}
