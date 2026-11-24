//! 侧边栏「工作区」面板（默认停靠左侧 dock）。
//!
//! 显示两组列表：
//! - 「已保存」：`workspaces.json` 中的工作区，按最近打开时间倒序；
//! - 「最近打开」：workspace 数据库中的最近项目（排除已保存的），悬停可「保存为工作区」。
//!
//! 单击在当前窗口打开，Ctrl+单击（macOS 为 Cmd+单击）在新窗口打开，右键菜单提供
//! 在新窗口打开 / 重命名 / 删除。启动页的工作区区块见 [`welcome`]，SSH 远程入口见 [`remote`]（M5）。

mod entries;
mod name_modal;
pub mod remote;
mod welcome;

use gpui::{
    Action, App, AsyncWindowContext, ClickEvent, Context, DismissEvent, Entity, EventEmitter,
    FocusHandle, Focusable, MouseDownEvent, Pixels, Point, Render, Subscription, Task, WeakEntity,
    Window, actions, anchored, deferred, px,
};
use ui::{ContextMenu, ListSubHeader, PopoverMenu, Tab, Tooltip, prelude::*};
use wisp_workspaces::{SavedWorkspaceId, WorkspaceStore};
use workspace::{
    RecentWorkspace, Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

pub use entries::{
    add_local_workspace, open_local_paths, open_saved_workspace, rename_saved_workspace,
};
pub use name_modal::NameWorkspaceModal;

actions!(
    workspace_panel,
    [
        /// Toggles focus on the workspaces panel.
        ToggleFocus,
        /// Picks a local folder, saves it as a workspace and opens it.
        OpenLocalFolder,
        /// Opens a remote (SSH) workspace.
        OpenRemote,
    ]
);

const WORKSPACE_PANEL_KEY: &str = "WorkspacePanel";
const DEFAULT_WIDTH: Pixels = px(260.);

pub fn init(cx: &mut App) {
    wisp_workspaces::init(cx);
    welcome::register(cx);

    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<WorkspacePanel>(window, cx);
        });
        workspace.register_action(|_, _: &OpenLocalFolder, window, cx| {
            add_local_workspace(cx.entity().downgrade(), window, cx);
        });
        workspace.register_action(|_, _: &OpenRemote, window, cx| {
            remote::open_remote_dialog(cx.entity().downgrade(), window, cx);
        });
    })
    .detach();
}

pub struct WorkspacePanel {
    workspace: WeakEntity<Workspace>,
    store: Entity<WorkspaceStore>,
    focus_handle: FocusHandle,
    position: DockPosition,
    recent: Vec<RecentWorkspace>,
    context_menu: Option<(Entity<ContextMenu>, Point<Pixels>, Subscription)>,
    _subscriptions: Vec<Subscription>,
}

impl WorkspacePanel {
    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> anyhow::Result<Entity<Self>> {
        workspace.update_in(&mut cx, |workspace, window, cx| {
            Self::new(workspace, window, cx)
        })
    }

    fn new(
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        let store = WorkspaceStore::get_or_init(cx);
        let workspace_handle = cx.entity().downgrade();
        let panel = cx.new(|cx| {
            let subscriptions = vec![cx.observe(&store, |_, _, cx| cx.notify())];
            Self {
                workspace: workspace_handle,
                store,
                focus_handle: cx.focus_handle(),
                position: DockPosition::Left,
                recent: Vec::new(),
                context_menu: None,
                _subscriptions: subscriptions,
            }
        });
        let task = entries::load_recent_workspaces(workspace, cx);
        Self::apply_recent(&panel, task, window, cx);
        panel
    }

    fn apply_recent(
        panel: &Entity<Self>,
        task: Task<Vec<RecentWorkspace>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let panel = panel.downgrade();
        window
            .spawn(cx, async move |cx| {
                let recent = task.await;
                panel
                    .update(cx, |panel, cx| {
                        panel.recent = recent;
                        cx.notify();
                    })
                    .ok();
            })
            .detach();
    }

    fn deploy_context_menu(
        &mut self,
        position: Point<Pixels>,
        id: SavedWorkspaceId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.workspace.clone();
        let context_menu = ContextMenu::build(window, cx, |menu, _, _| {
            let open_workspace = workspace.clone();
            let rename_workspace = workspace.clone();
            menu.entry(i18n::t("Open in New Window"), None, move |window, cx| {
                open_saved_workspace(id, true, open_workspace.clone(), window, cx);
            })
            .entry(i18n::t("Rename"), None, move |window, cx| {
                rename_saved_workspace(id, rename_workspace.clone(), window, cx);
            })
            .separator()
            .entry(i18n::t("Delete"), None, move |window, cx| {
                entries::delete_saved_workspace(id, window, cx);
            })
        });
        window.focus(&context_menu.focus_handle(cx), cx);
        let subscription = cx.subscribe(&context_menu, |panel, _, _: &DismissEvent, cx| {
            panel.context_menu.take();
            cx.notify();
        });
        self.context_menu = Some((context_menu, position, subscription));
        cx.notify();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.clone();
        h_flex()
            .h(Tab::container_height(cx))
            .flex_none()
            .px_2()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(Label::new(i18n::t("Workspaces")).size(LabelSize::Small))
            .child(
                PopoverMenu::new("workspace-panel-add-menu")
                    .trigger_with_tooltip(
                        IconButton::new("workspace-panel-add", IconName::Plus)
                            .icon_size(IconSize::Small),
                        Tooltip::text(i18n::t("Add Workspace")),
                    )
                    .anchor(gpui::Anchor::TopRight)
                    .menu(move |window, cx| {
                        let local_workspace = workspace.clone();
                        let remote_workspace = workspace.clone();
                        Some(ContextMenu::build(window, cx, |menu, _, _| {
                            menu.entry(
                                i18n::t("Open Local Folder…"),
                                Some(OpenLocalFolder.boxed_clone()),
                                move |window, cx| {
                                    add_local_workspace(local_workspace.clone(), window, cx)
                                },
                            )
                            .entry(
                                i18n::t("Open Remote…"),
                                Some(OpenRemote.boxed_clone()),
                                move |window, cx| {
                                    remote::open_remote_dialog(remote_workspace.clone(), window, cx)
                                },
                            )
                        }))
                    }),
            )
    }

    fn render_empty_hint(text: &'static str) -> impl IntoElement {
        div().px_3().py_1().child(
            Label::new(i18n::t(text))
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
    }
}

impl Render for WorkspacePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let saved = entries::saved_entries(store, None);
        let current = self
            .workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).database_id());
        let recent =
            entries::recent_entries(&self.recent, store, current, entries::MAX_RECENT_ENTRIES);

        let saved_rows: Vec<_> = saved
            .into_iter()
            .map(|entry| {
                let id = entry.id;
                entries::render_entry_row(
                    SharedString::from(format!("saved-{id}")),
                    entry.is_remote,
                    entry.name,
                    entry.detail,
                )
                .on_click(cx.listener(move |panel, event: &ClickEvent, window, cx| {
                    let new_window = entries::wants_new_window(event, cx);
                    open_saved_workspace(id, new_window, panel.workspace.clone(), window, cx);
                }))
                .on_secondary_mouse_down(cx.listener(
                    move |panel, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        panel.deploy_context_menu(event.position, id, window, cx);
                    },
                ))
                .into_any_element()
            })
            .collect();

        let recent_rows: Vec<_> = recent
            .into_iter()
            .enumerate()
            .map(|(ix, entry)| {
                let can_save = entry.can_save();
                let click_entry = entry.clone();
                let save_entry = entry.clone();
                entries::render_entry_row(
                    ("recent", ix),
                    entry.is_remote(),
                    entry.name,
                    entry.detail,
                )
                .when(can_save, |item| {
                    item.end_slot(
                        IconButton::new(("save-recent", ix), IconName::Star)
                            .icon_size(IconSize::Small)
                            .icon_color(Color::Muted)
                            .tooltip(Tooltip::text(i18n::t("Save as Workspace")))
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                cx.stop_propagation();
                                entries::save_recent_entry(
                                    &save_entry,
                                    panel.workspace.clone(),
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .show_end_slot_on_hover()
                })
                .on_click(cx.listener(move |panel, event: &ClickEvent, window, cx| {
                    let new_window = entries::wants_new_window(event, cx);
                    entries::open_recent_entry(
                        &click_entry,
                        new_window,
                        panel.workspace.clone(),
                        window,
                        cx,
                    );
                }))
                .into_any_element()
            })
            .collect();

        v_flex()
            .id("workspace-panel")
            .key_context("WorkspacePanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().colors().panel_background)
            .child(self.render_header(cx))
            .child(
                div()
                    .id("workspace-panel-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .py_1()
                            .child(ListSubHeader::new(i18n::t("Saved")).inset(true))
                            .when(saved_rows.is_empty(), |this| {
                                this.child(Self::render_empty_hint(
                                    "No saved workspaces yet. Open a local folder to add one.",
                                ))
                            })
                            .children(saved_rows)
                            .child(ListSubHeader::new(i18n::t("Recently Opened")).inset(true))
                            .when(recent_rows.is_empty(), |this| {
                                this.child(Self::render_empty_hint("No recent projects"))
                            })
                            .children(recent_rows),
                    ),
            )
            .children(self.context_menu.as_ref().map(|(menu, position, _)| {
                deferred(
                    anchored()
                        .position(*position)
                        .anchor(gpui::Anchor::TopLeft)
                        .child(menu.clone()),
                )
                .with_priority(1)
            }))
    }
}

impl Focusable for WorkspacePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for WorkspacePanel {}

impl Panel for WorkspacePanel {
    fn persistent_name() -> &'static str {
        "Workspace Panel"
    }

    fn panel_key() -> &'static str {
        WORKSPACE_PANEL_KEY
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        self.position
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(&mut self, position: DockPosition, _: &mut Window, cx: &mut Context<Self>) {
        self.position = position;
        cx.notify();
    }

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        DEFAULT_WIDTH
    }

    fn icon(&self, _: &Window, _: &App) -> Option<IconName> {
        Some(IconName::Folder)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some(i18n::t("Workspaces Panel"))
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !active {
            return;
        }
        // 每次展开面板时重新读取最近项目，以反映刚打开 / 关闭的项目。
        let panel = cx.entity();
        let workspace = self.workspace.clone();
        cx.defer_in(window, move |_, window, cx| {
            if let Some(workspace) = workspace.upgrade() {
                let task = workspace.update(cx, |workspace, cx| {
                    entries::load_recent_workspaces(workspace, cx)
                });
                Self::apply_recent(&panel, task, window, cx);
            }
        });
    }

    fn activation_priority(&self) -> u32 {
        // 项目面板 1、终端 2、Git 3、大纲 6；工作区面板取未占用的 4。
        4
    }
}
