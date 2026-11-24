//! Wisp 工作区数据层。
//!
//! - [`model`]：主机（[`Host`]）、已保存工作区（[`SavedWorkspace`]）等数据模型；
//! - [`file`]：`<config_dir>/workspaces.json` 的读写（原子写入、损坏时备份为 `.bak`）；
//! - [`WorkspaceStore`]：gpui 全局实体，提供增删改查并在变更时发出 [`WorkspaceStoreEvent`]；
//! - [`credentials`]：主机密码 / 私钥口令的系统凭据读写，key 为 `wisp-host://<host_id>`。
//!
//! 界面（侧边栏面板、启动页区块）在 `workspace_panel` crate 中；SSH 连接（M5）复用
//! `recent_projects` / `remote`，通过本 crate 读取主机信息与凭据。

pub mod credentials;
pub mod file;
pub mod model;

use std::path::{Path, PathBuf};

use chrono::Utc;
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global};

pub use credentials::{delete_host_secret, host_secret_url, load_host_secret, save_host_secret};
pub use file::{WorkspacesFile, load_file, save_file, workspaces_file_path};
pub use model::{
    AuthMethod, DEFAULT_SSH_PORT, Host, HostId, SavedWorkspace, SavedWorkspaceId, WorkspaceKind,
    WorkspaceLocation, same_local_path,
};

/// 初始化全局 [`WorkspaceStore`]（从 `workspaces.json` 同步加载，文件很小）。
pub fn init(cx: &mut App) {
    WorkspaceStore::get_or_init(cx);
}

/// [`WorkspaceStore`] 发出的事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceStoreEvent {
    /// 主机或工作区列表发生了变化（增删改、打开时间更新、重新加载）。
    Changed,
}

struct GlobalWorkspaceStore(Entity<WorkspaceStore>);

impl Global for GlobalWorkspaceStore {}

/// 已保存的主机与工作区，持久化到 `workspaces.json`。
///
/// 每次修改都会立即写回文件（先写临时文件再 rename），并发出
/// [`WorkspaceStoreEvent::Changed`]；界面可以 `cx.subscribe` 或 `cx.observe` 它来刷新。
pub struct WorkspaceStore {
    path: PathBuf,
    data: WorkspacesFile,
}

impl EventEmitter<WorkspaceStoreEvent> for WorkspaceStore {}

impl WorkspaceStore {
    /// 返回全局实体；尚未初始化时为 `None`。
    pub fn try_global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalWorkspaceStore>()
            .map(|global| global.0.clone())
    }

    /// 返回全局实体，必要时从默认路径加载并注册。
    pub fn get_or_init(cx: &mut App) -> Entity<Self> {
        if let Some(store) = Self::try_global(cx) {
            return store;
        }
        let store = cx.new(|_| Self::load(workspaces_file_path()));
        cx.set_global(GlobalWorkspaceStore(store.clone()));
        store
    }

    /// 从指定路径加载（文件不存在视为空；解析失败会备份为 `.bak` 并视为空）。
    pub fn load(path: PathBuf) -> Self {
        let data = load_file(&path);
        Self { path, data }
    }

    /// 数据文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 当前数据的只读视图。
    pub fn data(&self) -> &WorkspacesFile {
        &self.data
    }

    pub fn hosts(&self) -> &[Host] {
        &self.data.hosts
    }

    pub fn host(&self, id: HostId) -> Option<&Host> {
        self.data.host(id)
    }

    /// 按地址查找主机（主机名、端口、用户名都相同）。
    pub fn find_host(&self, host: &str, port: u16, username: &str) -> Option<&Host> {
        self.data.find_host(host, port, username)
    }

    pub fn workspaces(&self) -> &[SavedWorkspace] {
        &self.data.workspaces
    }

    pub fn workspace(&self, id: SavedWorkspaceId) -> Option<&SavedWorkspace> {
        self.data.workspace(id)
    }

    /// 按 `last_opened_at` 倒序（从未打开过的排在最后，同组内按名称）。
    pub fn workspaces_by_last_opened(&self) -> Vec<&SavedWorkspace> {
        self.data.workspaces_by_last_opened()
    }

    pub fn find_local_workspace(&self, path: &Path) -> Option<&SavedWorkspace> {
        self.data.find_local_workspace(path)
    }

    pub fn find_remote_workspace(&self, host_id: HostId, path: &str) -> Option<&SavedWorkspace> {
        self.data.find_remote_workspace(host_id, path)
    }

    /// 新增工作区，返回其 id。
    pub fn add_workspace(
        &mut self,
        name: impl Into<String>,
        location: WorkspaceLocation,
        cx: &mut Context<Self>,
    ) -> SavedWorkspaceId {
        let workspace = SavedWorkspace::new(name, location);
        let id = workspace.id;
        self.data.workspaces.push(workspace);
        self.changed(cx);
        id
    }

    /// 修改工作区；找不到时返回 `false`。
    pub fn update_workspace(
        &mut self,
        id: SavedWorkspaceId,
        update: impl FnOnce(&mut SavedWorkspace),
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(workspace) = self.data.workspace_mut(id) else {
            return false;
        };
        update(workspace);
        self.changed(cx);
        true
    }

    pub fn rename_workspace(
        &mut self,
        id: SavedWorkspaceId,
        name: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let name = name.into();
        self.update_workspace(id, |workspace| workspace.name = name, cx)
    }

    /// 把工作区的 `last_opened_at` 更新为当前时间。
    pub fn mark_opened(&mut self, id: SavedWorkspaceId, cx: &mut Context<Self>) -> bool {
        self.update_workspace(
            id,
            |workspace| workspace.last_opened_at = Some(Utc::now()),
            cx,
        )
    }

    /// 删除工作区记录（不删除磁盘上的文件）。
    pub fn remove_workspace(
        &mut self,
        id: SavedWorkspaceId,
        cx: &mut Context<Self>,
    ) -> Option<SavedWorkspace> {
        let index = self.data.workspaces.iter().position(|w| w.id == id)?;
        let removed = self.data.workspaces.remove(index);
        self.changed(cx);
        Some(removed)
    }

    /// 新增主机，返回其 id。凭据需另行调用 [`save_host_secret`] 保存。
    pub fn add_host(&mut self, host: Host, cx: &mut Context<Self>) -> HostId {
        let id = host.id;
        self.data.hosts.push(host);
        self.changed(cx);
        id
    }

    /// 用 `host.id` 匹配并整体替换主机信息；找不到时返回 `false`。
    pub fn update_host(&mut self, host: Host, cx: &mut Context<Self>) -> bool {
        let Some(existing) = self.data.hosts.iter_mut().find(|h| h.id == host.id) else {
            return false;
        };
        *existing = host;
        self.changed(cx);
        true
    }

    /// 删除主机及绑定在它上面的远程工作区记录。凭据需调用方另行 [`delete_host_secret`]。
    pub fn remove_host(&mut self, id: HostId, cx: &mut Context<Self>) -> Option<Host> {
        let index = self.data.hosts.iter().position(|h| h.id == id)?;
        let removed = self.data.hosts.remove(index);
        self.data
            .workspaces
            .retain(|workspace| workspace.host_id() != Some(id));
        self.changed(cx);
        Some(removed)
    }

    /// 从磁盘重新加载（例如用户手动编辑了 workspaces.json）。
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.data = load_file(&self.path);
        cx.emit(WorkspaceStoreEvent::Changed);
        cx.notify();
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        // 文件只有几 KB，同步写入可以保证退出前不会丢失最后一次修改。
        if let Err(error) = save_file(&self.path, &self.data) {
            log::error!(
                "failed to save workspaces to {}: {error:#}",
                self.path.display()
            );
        }
        cx.emit(WorkspaceStoreEvent::Changed);
        cx.notify();
    }
}
