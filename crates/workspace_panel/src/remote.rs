//! SSH 远程工作区（M5）。
//!
//! - [`open_remote_dialog`]：「打开远程」对话框（选择 / 新建主机 → 测试连接 → 浏览远程目录 →
//!   命名保存 → 打开），见 [`dialog`]；
//! - [`open_remote_workspace`]：用已保存的主机打开远程工作区；
//! - [`open_recent_ssh_project`]：打开「最近打开」中的 SSH 项目，地址与已保存主机一致时同样注入凭据。
//!
//! 连接复用 Zed 的 `recent_projects::open_remote_project`（`RemoteConnectionOptions::Ssh`），
//! 密码 / 私钥口令从系统凭据管理器读出后放进 `SshConnectionOptions::password`，由
//! `remote_connection::RemoteClientDelegate` 自动应答 ssh 的 askpass 提示；凭据缺失或被拒绝时
//! 回退到 Zed 原有的密码输入框。私钥通过 `-i <path> -o IdentitiesOnly=yes` 传给 ssh。
//! 「测试连接」与目录浏览用 [`ssh_command`] 直接调用系统 ssh（Zed 的连接层必须上传远程服务端）。

mod dialog;
mod ssh_command;

use std::path::PathBuf;

use ::remote::{RemoteConnectionOptions, SshConnectionOptions};
use anyhow::Result;
use gpui::{App, AsyncApp, WeakEntity, Window};
use recent_projects::RemoteSettings;
use settings::Settings as _;
use util::ResultExt as _;
use wisp_workspaces::{
    AuthMethod, DEFAULT_SSH_PORT, Host, SavedWorkspaceId, WorkspaceStore, load_host_secret,
};
use workspace::{
    AppState, MultiWorkspace, OpenOptions, Toast, Workspace, notifications::DetachAndPromptErr as _,
    notifications::NotificationId,
};

pub use dialog::RemoteDialog;

struct RemoteHostMissingToast;

/// 打开「打开远程」对话框。
pub fn open_remote_dialog(workspace: WeakEntity<Workspace>, window: &mut Window, cx: &mut App) {
    let Some(workspace_entity) = workspace.upgrade() else {
        return;
    };
    workspace_entity.update(cx, |workspace_ref, cx| {
        workspace_ref.toggle_modal(window, cx, move |window, cx| {
            RemoteDialog::new(workspace, window, cx)
        });
    });
}

/// 私钥认证时传给 ssh 的参数。
fn key_args(host: &Host) -> Option<Vec<String>> {
    if host.auth != AuthMethod::Key {
        return None;
    }
    let key_path = host.key_path.as_ref()?;
    Some(vec![
        "-i".to_string(),
        key_path.to_string_lossy().into_owned(),
        "-o".to_string(),
        "IdentitiesOnly=yes".to_string(),
    ])
}

/// 由已保存的主机构造 Zed 的 SSH 连接参数；`secret` 为密码或私钥口令。
pub fn ssh_connection_options(host: &Host, secret: Option<String>) -> SshConnectionOptions {
    SshConnectionOptions {
        host: host.host.clone().into(),
        username: (!host.username.is_empty()).then(|| host.username.clone()),
        // 与手动输入 `user@host` 时一致：默认端口不写入，便于和「最近打开」中的记录匹配。
        port: (host.port != DEFAULT_SSH_PORT).then_some(host.port),
        password: secret.filter(|secret| !secret.is_empty()),
        args: key_args(host),
        nickname: (!host.name.trim().is_empty()).then(|| host.name.clone()),
        ..Default::default()
    }
}

fn show_host_missing(workspace: &WeakEntity<Workspace>, cx: &mut App) {
    workspace
        .update(cx, |workspace, cx| {
            workspace.show_toast(
                Toast::new(
                    NotificationId::unique::<RemoteHostMissingToast>(),
                    i18n::t("The host of this workspace no longer exists. Please add it again."),
                )
                .autohide(),
                cx,
            );
        })
        .ok();
}

/// 打开一个已保存的远程工作区。
///
/// - `workspace_id`：已保存工作区 id，连接成功后更新其 `last_opened_at`；
/// - `host`：工作区绑定的主机，`None` 表示主机记录已丢失；
/// - `path`：远程目录（POSIX 路径）；
/// - `new_window`：是否在新窗口打开（否则替换当前窗口的项目）。
pub fn open_remote_workspace(
    workspace: WeakEntity<Workspace>,
    workspace_id: SavedWorkspaceId,
    host: Option<Host>,
    path: String,
    new_window: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(host) = host else {
        show_host_missing(&workspace, cx);
        return;
    };
    open_host_path(
        workspace,
        host,
        path,
        Some(workspace_id),
        new_window,
        window,
        cx,
    );
}

/// 读取主机凭据后打开 `host` 上的 `path`。
pub(crate) fn open_host_path(
    workspace: WeakEntity<Workspace>,
    host: Host,
    path: String,
    saved_id: Option<SavedWorkspaceId>,
    new_window: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(app_state) = workspace
        .read_with(cx, |workspace, _| workspace.app_state().clone())
        .ok()
    else {
        return;
    };
    let requesting_window = requesting_window(new_window, window);
    let secret = load_host_secret(host.id, cx);
    cx.spawn(async move |cx: &mut AsyncApp| {
        // 读取失败（或从未保存）时不注入密码，连接时由 Zed 弹出密码框。
        let secret = secret.await.log_err().flatten();
        let options = ssh_connection_options(&host, secret);
        open_ssh(
            options,
            vec![PathBuf::from(&path)],
            app_state,
            requesting_window,
            saved_id.map(|id| (id, path)),
            cx,
        )
        .await
    })
    .detach_and_prompt_err(i18n::t("Failed to open project"), window, cx, |_, _, _| None);
}

/// 打开「最近打开」中的 SSH 项目：沿用 Zed 原有方式（含 settings 里的 ssh_connections 参数）；
/// 若地址（host + port + user）与某个已保存主机一致，则同样注入其私钥参数与保存的凭据。
pub fn open_recent_ssh_project(
    mut options: SshConnectionOptions,
    paths: Vec<PathBuf>,
    new_window: bool,
    workspace: WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(app_state) = workspace
        .read_with(cx, |workspace, _| workspace.app_state().clone())
        .ok()
    else {
        return;
    };
    RemoteSettings::get_global(cx).fill_connection_options_from_settings(&mut options);
    let saved_host = WorkspaceStore::get_or_init(cx)
        .read(cx)
        .find_host(
            &options.host.to_string(),
            options.port.unwrap_or(DEFAULT_SSH_PORT),
            options.username.as_deref().unwrap_or_default(),
        )
        .cloned();
    let secret = saved_host.as_ref().map(|host| load_host_secret(host.id, cx));
    if let Some(host) = &saved_host {
        let has_identity = options
            .args
            .as_ref()
            .is_some_and(|args| args.iter().any(|arg| arg.starts_with("-i")));
        if !has_identity && let Some(extra) = key_args(host) {
            options.args.get_or_insert_with(Vec::new).extend(extra);
        }
    }
    let requesting_window = requesting_window(new_window, window);
    cx.spawn(async move |cx: &mut AsyncApp| {
        if let Some(secret) = secret {
            options.password = secret.await.log_err().flatten().filter(|s| !s.is_empty());
        }
        open_ssh(options, paths, app_state, requesting_window, None, cx).await
    })
    .detach_and_prompt_err(i18n::t("Failed to open project"), window, cx, |_, _, _| None);
}

fn requesting_window(
    new_window: bool,
    window: &mut Window,
) -> Option<gpui::WindowHandle<MultiWorkspace>> {
    if new_window {
        None
    } else {
        window.window_handle().downcast::<MultiWorkspace>()
    }
}

/// 通过 Zed 的远程打开流程（含连接进度 / 密码提示框、失败重试）打开项目；
/// 打开成功后为已保存工作区调用 `mark_opened`。
async fn open_ssh(
    options: SshConnectionOptions,
    paths: Vec<PathBuf>,
    app_state: std::sync::Arc<AppState>,
    requesting_window: Option<gpui::WindowHandle<MultiWorkspace>>,
    saved: Option<(SavedWorkspaceId, String)>,
    cx: &mut AsyncApp,
) -> Result<()> {
    let window = recent_projects::open_remote_project(
        RemoteConnectionOptions::Ssh(options),
        paths,
        app_state,
        OpenOptions {
            requesting_window,
            ..Default::default()
        },
        cx,
    )
    .await?;

    let Some((id, path)) = saved else {
        return Ok(());
    };
    // open_remote_project 在用户取消或连接失败时也返回 Ok，需检查窗口里是否真的打开了该远程目录。
    let opened = window
        .update(cx, |multi_workspace, _, cx| {
            let cx: &App = cx;
            let project = multi_workspace.workspace().read(cx).project().read(cx);
            project.remote_connection_options(cx).is_some()
                && project.visible_worktrees(cx).any(|worktree| {
                    worktree
                        .read(cx)
                        .abs_path()
                        .to_string_lossy()
                        .replace('\\', "/")
                        .trim_end_matches('/')
                        == path.trim_end_matches('/')
                })
        })
        .unwrap_or(false);
    if opened {
        cx.update(|cx| {
            WorkspaceStore::get_or_init(cx).update(cx, |store, cx| store.mark_opened(id, cx));
        });
    }
    Ok(())
}
