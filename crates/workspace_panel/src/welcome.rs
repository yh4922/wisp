//! 启动页（welcome / launchpad）中的「工作区」区块，通过
//! [`workspace::welcome::WelcomeSectionProvider`] 注入到 `workspace` crate 的启动页中。

use std::sync::Arc;

use gpui::{App, AppContext as _, Context, Entity, Render, Subscription, WeakEntity, Window};
use ui::{Divider, DividerColor, prelude::*};
use wisp_workspaces::WorkspaceStore;
use workspace::{RecentWorkspace, Workspace, welcome::WelcomeSectionProvider};

use crate::entries::{self, render_entry_row, wants_new_window};
use crate::remote;

/// 启动页「已保存的工作区」最多显示的条数。
const MAX_WELCOME_SAVED: usize = 8;
/// 启动页「最近打开」最多显示的条数。
const MAX_WELCOME_RECENT: usize = 5;

/// 注册启动页区块。
pub(crate) fn register(cx: &mut App) {
    cx.set_global(WelcomeSectionProvider(Arc::new(|workspace, window, cx| {
        cx.new(|cx| WelcomeWorkspaces::new(workspace, window, cx))
            .into()
    })));
}

pub struct WelcomeWorkspaces {
    workspace: WeakEntity<Workspace>,
    store: Entity<WorkspaceStore>,
    recent: Vec<RecentWorkspace>,
    _subscription: Subscription,
}

impl WelcomeWorkspaces {
    fn new(workspace: WeakEntity<Workspace>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = WorkspaceStore::get_or_init(cx);
        let subscription = cx.observe(&store, |_, _, cx| cx.notify());

        if let Some(workspace) = workspace.upgrade() {
            let task = workspace.update(cx, |workspace, cx| {
                entries::load_recent_workspaces(workspace, cx)
            });
            cx.spawn(async move |this, cx| {
                let recent = task.await;
                this.update(cx, |this, cx| {
                    this.recent = recent;
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }

        Self {
            workspace,
            store,
            recent: Vec::new(),
            _subscription: subscription,
        }
    }
}

fn section_header(title: &'static str, cx: &App) -> impl IntoElement {
    h_flex()
        .px_1()
        .mb_2()
        .gap_2()
        .child(
            Label::new(i18n::t(title).to_uppercase())
                .buffer_font(cx)
                .color(Color::Muted)
                .size(LabelSize::XSmall),
        )
        .child(Divider::horizontal().color(DividerColor::BorderVariant))
}

impl Render for WelcomeWorkspaces {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let saved = entries::saved_entries(store, Some(MAX_WELCOME_SAVED));
        let current = self
            .workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).database_id());
        let recent = entries::recent_entries(&self.recent, store, current, MAX_WELCOME_RECENT);

        let workspace = self.workspace.clone();
        let buttons = h_flex()
            .w_full()
            .gap_2()
            .child(
                Button::new("welcome-open-local-folder", i18n::t("Open Local Folder"))
                    .start_icon(Icon::new(IconName::FolderOpen).size(IconSize::Small))
                    .style(ButtonStyle::Outlined)
                    .on_click({
                        let workspace = workspace.clone();
                        move |_, window, cx| {
                            entries::add_local_workspace(workspace.clone(), window, cx)
                        }
                    }),
            )
            .child(
                Button::new("welcome-open-remote", i18n::t("Open Remote"))
                    .start_icon(Icon::new(IconName::Server).size(IconSize::Small))
                    .style(ButtonStyle::Outlined)
                    .on_click({
                        let workspace = workspace.clone();
                        move |_, window, cx| {
                            remote::open_remote_dialog(workspace.clone(), window, cx)
                        }
                    }),
            );

        let saved_rows = saved.into_iter().map(|entry| {
            let id = entry.id;
            let workspace = workspace.clone();
            render_entry_row(
                SharedString::from(format!("welcome-saved-{id}")),
                entry.is_remote,
                entry.name,
                entry.detail,
            )
            .on_click(move |event, window, cx| {
                let new_window = wants_new_window(event, cx);
                entries::open_saved_workspace(id, new_window, workspace.clone(), window, cx);
            })
            .into_any_element()
        });

        let recent_rows = recent.into_iter().map(|entry| {
            let workspace = workspace.clone();
            render_entry_row(
                SharedString::from(format!("welcome-recent-{}", i64::from(entry.workspace_id))),
                entry.is_remote(),
                entry.name.clone(),
                entry.detail.clone(),
            )
            .on_click(move |event, window, cx| {
                let new_window = wants_new_window(event, cx);
                entries::open_recent_entry(&entry, new_window, workspace.clone(), window, cx);
            })
            .into_any_element()
        });

        let saved_rows: Vec<_> = saved_rows.collect();
        let recent_rows: Vec<_> = recent_rows.collect();

        v_flex()
            .w_full()
            .gap_6()
            .child(
                v_flex()
                    .w_full()
                    .child(section_header("Workspaces", cx))
                    .child(buttons),
            )
            .child(
                v_flex()
                    .w_full()
                    .child(section_header("Saved Workspaces", cx))
                    .when(saved_rows.is_empty(), |this| {
                        this.child(
                            div().px_1().child(
                                Label::new(i18n::t(
                                    "No saved workspaces yet. Open a local folder to add one.",
                                ))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                            ),
                        )
                    })
                    .children(saved_rows),
            )
            .when(!recent_rows.is_empty(), |this| {
                this.child(
                    v_flex()
                        .w_full()
                        .child(section_header("Recently Opened", cx))
                        .children(recent_rows),
                )
            })
    }
}
