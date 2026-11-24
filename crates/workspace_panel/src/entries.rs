//! 面板与启动页共用的逻辑：列表数据（已保存 / 最近打开）、条目渲染、打开与保存操作。

use std::path::{Path, PathBuf};

use ::remote::RemoteConnectionOptions;
use gpui::{App, AsyncApp, ClickEvent, PathPromptOptions, TaskExt as _, WeakEntity, Window};
use ui::{ListItem, ListItemSpacing, prelude::*};
use util::{ResultExt as _, paths::PathExt as _};
use wisp_workspaces::{
    AuthMethod, DEFAULT_SSH_PORT, Host, SavedWorkspace, SavedWorkspaceId, WorkspaceLocation,
    WorkspaceStore,
};
use workspace::{
    MultiWorkspace, OpenMode, OpenOptions, RecentWorkspace, SerializedWorkspaceLocation, Workspace,
    WorkspaceDb, WorkspaceId, notifications::DetachAndPromptErr as _,
};

use crate::name_modal::NameWorkspaceModal;
use crate::remote;

/// 「最近打开」最多显示的条数。
pub const MAX_RECENT_ENTRIES: usize = 10;

/// 「已保存」列表中一行的显示数据。
#[derive(Clone, Debug)]
pub struct SavedEntry {
    pub id: SavedWorkspaceId,
    pub name: SharedString,
    pub detail: SharedString,
    pub is_remote: bool,
}

/// 「最近打开」列表中一行的显示数据。
#[derive(Clone, Debug)]
pub struct RecentEntry {
    pub workspace_id: WorkspaceId,
    pub name: SharedString,
    pub detail: SharedString,
    pub location: SerializedWorkspaceLocation,
    pub paths: Vec<PathBuf>,
}

impl RecentEntry {
    pub fn is_remote(&self) -> bool {
        matches!(self.location, SerializedWorkspaceLocation::Remote(_))
    }

    /// 只有单个目录的本地 / SSH 项目可以保存为工作区。
    pub fn can_save(&self) -> bool {
        self.paths.len() == 1
    }
}

/// 已保存工作区，按最近打开时间倒序。
pub fn saved_entries(store: &WorkspaceStore, limit: Option<usize>) -> Vec<SavedEntry> {
    store
        .workspaces_by_last_opened()
        .into_iter()
        .take(limit.unwrap_or(usize::MAX))
        .map(|workspace| saved_entry(store, workspace))
        .collect()
}

fn saved_entry(store: &WorkspaceStore, workspace: &SavedWorkspace) -> SavedEntry {
    let (detail, is_remote) = match &workspace.location {
        WorkspaceLocation::Local { path } => (path.compact().to_string_lossy().to_string(), false),
        WorkspaceLocation::Remote { host_id, path } => {
            let address = store
                .host(*host_id)
                .map(Host::address)
                .unwrap_or_else(|| "?".to_string());
            (format!("{address}:{path}"), true)
        }
    };
    SavedEntry {
        id: workspace.id,
        name: workspace.name.clone().into(),
        detail: detail.into(),
        is_remote,
    }
}

/// 从 workspace 数据库读取最近打开的项目（与「打开最近项目」同源）。
pub fn load_recent_workspaces(
    workspace: &Workspace,
    cx: &mut App,
) -> gpui::Task<Vec<RecentWorkspace>> {
    let fs = workspace.app_state().fs.clone();
    let db = WorkspaceDb::global(cx);
    cx.spawn(async move |_: &mut AsyncApp| {
        db.recent_project_workspaces(fs.as_ref())
            .await
            .log_err()
            .unwrap_or_default()
    })
}

/// 把数据库中的最近项目转换为列表行：排除当前工作区、已保存的条目以及非 SSH 的远程项目。
pub fn recent_entries(
    recent: &[RecentWorkspace],
    store: &WorkspaceStore,
    current: Option<WorkspaceId>,
    limit: usize,
) -> Vec<RecentEntry> {
    recent
        .iter()
        .filter(|workspace| Some(workspace.workspace_id) != current)
        .filter_map(|workspace| {
            let paths = workspace.paths.paths().to_vec();
            let display_paths: Vec<&PathBuf> = workspace.identity_paths.ordered_paths().collect();
            let name = display_paths
                .iter()
                .filter_map(|path| path.file_name())
                .map(|name| name.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let detail = match &workspace.location {
                SerializedWorkspaceLocation::Local => {
                    if paths.len() == 1 && store.find_local_workspace(&paths[0]).is_some() {
                        return None;
                    }
                    display_paths
                        .iter()
                        .map(|path| path.compact().to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
                SerializedWorkspaceLocation::Remote(RemoteConnectionOptions::Ssh(options)) => {
                    let host = options.host.to_string();
                    let port = options.port.unwrap_or(DEFAULT_SSH_PORT);
                    let username = options.username.clone().unwrap_or_default();
                    if paths.len() == 1
                        && let Some(saved_host) = store.find_host(&host, port, &username)
                        && store
                            .find_remote_workspace(saved_host.id, &remote_path(&paths[0]))
                            .is_some()
                    {
                        return None;
                    }
                    let address =
                        Host::new("", host, port, username, AuthMethod::Password, None).address();
                    let joined = display_paths
                        .iter()
                        .map(|path| remote_path(path))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{address}:{joined}")
                }
                SerializedWorkspaceLocation::Remote(_) => return None,
            };
            Some(RecentEntry {
                workspace_id: workspace.workspace_id,
                name: if name.is_empty() {
                    detail.clone().into()
                } else {
                    name.into()
                },
                detail: detail.into(),
                location: workspace.location.clone(),
                paths,
            })
        })
        .take(limit)
        .collect()
}

fn remote_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 列表行：本地 / 远程图标 + 名称，第二行较淡的路径。
pub fn render_entry_row(
    id: impl Into<ElementId>,
    is_remote: bool,
    name: SharedString,
    detail: SharedString,
) -> ListItem {
    let icon = if is_remote {
        IconName::Server
    } else {
        IconName::Folder
    };
    ListItem::new(id)
        .inset(true)
        .spacing(ListItemSpacing::Sparse)
        .start_slot(Icon::new(icon).color(Color::Muted).size(IconSize::Small))
        .child(
            v_flex()
                .min_w_0()
                .w_full()
                .child(Label::new(name).truncate())
                .child(
                    Label::new(detail)
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .truncate(),
                ),
        )
}

/// Ctrl+单击（macOS 上 Cmd+单击）表示在新窗口打开。
pub fn wants_new_window(event: &ClickEvent, cx: &App) -> bool {
    event.modifiers().secondary(cx)
}

/// 打开本地目录：`new_window` 为 false 时替换当前窗口的项目（与「打开最近项目」相同，
/// 由 `MultiWorkspace::open_project` 处理当前窗口未保存修改的提示）。
pub fn open_local_paths(
    paths: Vec<PathBuf>,
    new_window: bool,
    workspace: WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) {
    if new_window {
        workspace
            .update(cx, |workspace, cx| {
                workspace
                    .open_workspace_for_paths(OpenMode::NewWindow, paths, window, cx)
                    .detach_and_prompt_err(
                        i18n::t("Failed to open project"),
                        window,
                        cx,
                        |_, _, _| None,
                    );
            })
            .log_err();
    } else if let Some(handle) = window.window_handle().downcast::<MultiWorkspace>() {
        cx.defer(move |cx| {
            if let Some(task) = handle
                .update(cx, |multi_workspace, window, cx| {
                    multi_workspace.open_project(paths, OpenMode::Activate, window, cx)
                })
                .log_err()
            {
                task.detach_and_log_err(cx);
            }
        });
    }
}

/// 打开已保存的工作区，本地工作区同时更新 `last_opened_at`；远程工作区交给
/// [`remote::open_remote_workspace`]（连接成功后更新）。
pub fn open_saved_workspace(
    id: SavedWorkspaceId,
    new_window: bool,
    workspace: WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) {
    let store = WorkspaceStore::get_or_init(cx);
    let Some(saved) = store.read(cx).workspace(id).cloned() else {
        return;
    };
    match saved.location {
        WorkspaceLocation::Local { path } => {
            store.update(cx, |store, cx| store.mark_opened(id, cx));
            open_local_paths(vec![path], new_window, workspace, window, cx);
        }
        WorkspaceLocation::Remote { host_id, path } => {
            let host = store.read(cx).host(host_id).cloned();
            remote::open_remote_workspace(workspace, id, host, path, new_window, window, cx);
        }
    }
}

/// 打开「最近打开」中的项目；远程项目沿用 `recent_projects` 的现有连接流程。
pub fn open_recent_entry(
    entry: &RecentEntry,
    new_window: bool,
    workspace: WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) {
    match entry.location.clone() {
        SerializedWorkspaceLocation::Local => {
            open_local_paths(entry.paths.clone(), new_window, workspace, window, cx);
        }
        SerializedWorkspaceLocation::Remote(RemoteConnectionOptions::Ssh(options)) => {
            remote::open_recent_ssh_project(
                options,
                entry.paths.clone(),
                new_window,
                workspace,
                window,
                cx,
            );
        }
        SerializedWorkspaceLocation::Remote(connection) => {
            let Some(app_state) = workspace
                .read_with(cx, |workspace, _| workspace.app_state().clone())
                .ok()
            else {
                return;
            };
            let requesting_window = if new_window {
                None
            } else {
                window.window_handle().downcast::<MultiWorkspace>()
            };
            let open_options = OpenOptions {
                requesting_window,
                ..Default::default()
            };
            let paths = entry.paths.clone();
            cx.spawn(async move |cx: &mut AsyncApp| {
                recent_projects::open_remote_project(connection, paths, app_state, open_options, cx)
                    .await
                    .map(|_| ())
            })
            .detach_and_prompt_err(
                i18n::t("Failed to open project"),
                window,
                cx,
                |_, _, _| None,
            );
        }
    }
}

fn default_name_for(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// 「打开本地文件夹…」：系统文件夹选择器 → 命名框（默认文件夹名）→ 保存并在当前窗口打开。
/// 已保存过的文件夹直接打开。
pub fn add_local_workspace(workspace: WeakEntity<Workspace>, window: &mut Window, cx: &mut App) {
    let paths = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    });
    window
        .spawn(cx, async move |cx| {
            let Some(path) = paths
                .await
                .ok()
                .and_then(|result| result.log_err())
                .flatten()
                .and_then(|paths| paths.into_iter().next())
            else {
                return;
            };
            cx.update(|window, cx| {
                let store = WorkspaceStore::get_or_init(cx);
                if let Some(existing) = store.read(cx).find_local_workspace(&path) {
                    let id = existing.id;
                    open_saved_workspace(id, false, workspace, window, cx);
                    return;
                }
                let Some(workspace_entity) = workspace.upgrade() else {
                    return;
                };
                let subtitle: SharedString = path.compact().to_string_lossy().to_string().into();
                let default_name = default_name_for(&path);
                workspace_entity.update(cx, |workspace_ref, cx| {
                    NameWorkspaceModal::show(
                        workspace_ref,
                        i18n::t("Save Workspace"),
                        Some(subtitle),
                        default_name,
                        move |name, window, cx| {
                            let id = store.update(cx, |store, cx| {
                                store.add_workspace(name, WorkspaceLocation::Local { path }, cx)
                            });
                            open_saved_workspace(id, false, workspace, window, cx);
                        },
                        window,
                        cx,
                    );
                });
            })
            .log_err();
        })
        .detach();
}

/// 把「最近打开」中的条目保存为工作区（弹出命名框）。
pub fn save_recent_entry(
    entry: &RecentEntry,
    workspace: WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(path) = entry.paths.first().cloned() else {
        return;
    };
    if !entry.can_save() {
        return;
    }
    let location = entry.location.clone();
    let Some(workspace_entity) = workspace.upgrade() else {
        return;
    };
    let store = WorkspaceStore::get_or_init(cx);
    let subtitle = entry.detail.clone();
    let default_name = match &location {
        SerializedWorkspaceLocation::Local => default_name_for(&path),
        SerializedWorkspaceLocation::Remote(_) => {
            let remote = remote_path(&path);
            remote
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .unwrap_or(&remote)
                .to_string()
        }
    };
    workspace_entity.update(cx, |workspace_ref, cx| {
        NameWorkspaceModal::show(
            workspace_ref,
            i18n::t("Save as Workspace"),
            Some(subtitle),
            default_name,
            move |name, _, cx| {
                store.update(cx, |store, cx| match location {
                    SerializedWorkspaceLocation::Local => {
                        store.add_workspace(name, WorkspaceLocation::Local { path }, cx);
                    }
                    SerializedWorkspaceLocation::Remote(RemoteConnectionOptions::Ssh(options)) => {
                        let host = options.host.to_string();
                        let port = options.port.unwrap_or(DEFAULT_SSH_PORT);
                        let username = options.username.clone().unwrap_or_default();
                        let host_id = match store.find_host(&host, port, &username) {
                            Some(existing) => existing.id,
                            None => store.add_host(
                                Host::new(
                                    options.nickname.clone().unwrap_or_default(),
                                    host,
                                    port,
                                    username,
                                    AuthMethod::Password,
                                    None,
                                ),
                                cx,
                            ),
                        };
                        store.add_workspace(
                            name,
                            WorkspaceLocation::Remote {
                                host_id,
                                path: remote_path(&path),
                            },
                            cx,
                        );
                    }
                    SerializedWorkspaceLocation::Remote(_) => {}
                });
            },
            window,
            cx,
        );
    });
}

/// 重命名已保存的工作区。
pub fn rename_saved_workspace(
    id: SavedWorkspaceId,
    workspace: WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) {
    let store = WorkspaceStore::get_or_init(cx);
    let Some(saved) = store.read(cx).workspace(id).cloned() else {
        return;
    };
    let Some(workspace_entity) = workspace.upgrade() else {
        return;
    };
    let subtitle = saved_entry(store.read(cx), &saved).detail;
    workspace_entity.update(cx, |workspace_ref, cx| {
        NameWorkspaceModal::show(
            workspace_ref,
            i18n::t("Rename Workspace"),
            Some(subtitle),
            saved.name.clone(),
            move |name, _, cx| {
                store.update(cx, |store, cx| store.rename_workspace(id, name, cx));
            },
            window,
            cx,
        );
    });
}

/// 删除已保存的工作区记录（先确认；不删除磁盘上的文件）。
pub fn delete_saved_workspace(id: SavedWorkspaceId, window: &mut Window, cx: &mut App) {
    let store = WorkspaceStore::get_or_init(cx);
    let Some(saved) = store.read(cx).workspace(id).cloned() else {
        return;
    };
    let message = i18n::tf!("Delete workspace \"{name}\"?", name = saved.name);
    let answer = window.prompt(
        gpui::PromptLevel::Warning,
        &message,
        Some(i18n::t(
            "Only the saved record is removed. Files on disk are not deleted.",
        )),
        &[i18n::t("Delete"), i18n::t("Cancel")],
        cx,
    );
    cx.spawn(async move |cx: &mut AsyncApp| {
        if answer.await.ok() == Some(0) {
            store.update(cx, |store, cx| {
                store.remove_workspace(id, cx);
            });
        }
    })
    .detach();
}
