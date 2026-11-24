//! 工作区命名弹窗：保存新工作区、重命名时输入备注名。

use editor::Editor;
use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Render, Subscription, Window,
};
use menu::{Cancel, Confirm};
use ui::prelude::*;
use workspace::{ModalView, Workspace};

type OnConfirm = Box<dyn FnOnce(String, &mut Window, &mut App)>;

/// 带一个单行输入框的命名弹窗。回车确认（名称为空时不关闭），Esc 取消。
pub struct NameWorkspaceModal {
    title: SharedString,
    subtitle: Option<SharedString>,
    editor: Entity<Editor>,
    on_confirm: Option<OnConfirm>,
    _editor_subscription: Subscription,
}

impl NameWorkspaceModal {
    /// 在 `workspace` 中弹出命名框，确认后以去除首尾空白的名称调用 `on_confirm`。
    pub fn show(
        workspace: &mut Workspace,
        title: impl Into<SharedString>,
        subtitle: Option<SharedString>,
        default_name: String,
        on_confirm: impl FnOnce(String, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut App,
    ) {
        let title = title.into();
        workspace.toggle_modal(window, cx, move |window, cx| {
            let editor = cx.new(|cx| {
                let mut editor = Editor::single_line(window, cx);
                editor.set_placeholder_text(i18n::t("Workspace name"), window, cx);
                editor.set_text(default_name, window, cx);
                editor.select_all(&editor::actions::SelectAll, window, cx);
                editor
            });
            let editor_subscription = cx.subscribe(&editor, |_, _, event, cx| {
                if let editor::EditorEvent::BufferEdited = event {
                    cx.notify();
                }
            });
            Self {
                title,
                subtitle,
                editor,
                on_confirm: Some(Box::new(on_confirm)),
                _editor_subscription: editor_subscription,
            }
        });
    }

    fn name(&self, cx: &App) -> String {
        self.editor.read(cx).text(cx).trim().to_string()
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name(cx);
        if name.is_empty() {
            return;
        }
        cx.emit(DismissEvent);
        if let Some(on_confirm) = self.on_confirm.take() {
            on_confirm(name, window, cx);
        }
    }
}

impl EventEmitter<DismissEvent> for NameWorkspaceModal {}

impl ModalView for NameWorkspaceModal {}

impl Focusable for NameWorkspaceModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.focus_handle(cx)
    }
}

impl Render for NameWorkspaceModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_empty = self.name(cx).is_empty();
        v_flex()
            .key_context("NameWorkspaceModal")
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::confirm))
            .elevation_2(cx)
            .w(rems(34.))
            .child(
                h_flex()
                    .px_3()
                    .pt_2()
                    .pb_1()
                    .w_full()
                    .gap_1p5()
                    .child(Icon::new(IconName::Folder).size(IconSize::XSmall))
                    .child(Headline::new(self.title.clone()).size(HeadlineSize::XSmall)),
            )
            .child(
                v_flex()
                    .px_3()
                    .pb_3()
                    .w_full()
                    .gap_2()
                    .child(self.editor.clone())
                    .when_some(self.subtitle.clone(), |this, subtitle| {
                        this.child(
                            Label::new(subtitle)
                                .size(LabelSize::Small)
                                .color(Color::Muted)
                                .single_line()
                                .truncate(),
                        )
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap_1()
                            .child(
                                Button::new("name-workspace-cancel", i18n::t("Cancel")).on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.cancel(&Cancel, window, cx)
                                    }),
                                ),
                            )
                            .child(
                                Button::new("name-workspace-confirm", i18n::t("Save"))
                                    .style(ButtonStyle::Filled)
                                    .disabled(is_empty)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.confirm(&Confirm, window, cx)
                                    })),
                            ),
                    ),
            )
    }
}
