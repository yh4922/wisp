//! 工作区与远程主机的数据模型（对应 `workspaces.json` 的结构）。

use std::fmt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// SSH 默认端口。
pub const DEFAULT_SSH_PORT: u16 = 22;

/// 远程主机 id。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HostId(pub Uuid);

impl HostId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for HostId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for HostId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// 已保存工作区 id。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SavedWorkspaceId(pub Uuid);

impl SavedWorkspaceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SavedWorkspaceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SavedWorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// SSH 认证方式。密码 / 私钥口令本身不写入 JSON，而是存放在系统凭据管理器中。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    /// 密码认证。
    #[default]
    Password,
    /// 私钥认证，私钥路径见 [`Host::key_path`]，口令（可选）存凭据管理器。
    Key,
}

fn default_ssh_port() -> u16 {
    DEFAULT_SSH_PORT
}

/// 远程主机（SSH）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    pub id: HostId,
    /// 备注名。
    #[serde(default)]
    pub name: String,
    /// 主机名或 IP。
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub auth: AuthMethod,
    /// 私钥路径，仅 [`AuthMethod::Key`] 时使用。
    #[serde(default)]
    pub key_path: Option<PathBuf>,
}

impl Host {
    pub fn new(
        name: impl Into<String>,
        host: impl Into<String>,
        port: u16,
        username: impl Into<String>,
        auth: AuthMethod,
        key_path: Option<PathBuf>,
    ) -> Self {
        Self {
            id: HostId::new(),
            name: name.into(),
            host: host.into(),
            port,
            username: username.into(),
            auth,
            key_path,
        }
    }

    /// `user@host`，端口不是 22 时为 `user@host:port`。
    pub fn address(&self) -> String {
        let mut address = if self.username.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.username, self.host)
        };
        if self.port != DEFAULT_SSH_PORT {
            address.push_str(&format!(":{}", self.port));
        }
        address
    }

    /// 界面上显示的名称：有备注名用备注名，否则用地址。
    pub fn display_name(&self) -> String {
        if self.name.trim().is_empty() {
            self.address()
        } else {
            self.name.clone()
        }
    }
}

/// 工作区类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkspaceKind {
    Local,
    Remote,
}

/// 工作区位置。序列化时以 `kind` 字段区分（`"local"` / `"remote"`），并与
/// [`SavedWorkspace`] 的其他字段平铺在同一个 JSON 对象中。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WorkspaceLocation {
    /// 本地文件夹。
    Local { path: PathBuf },
    /// 远程主机上的目录（远程路径一律按 POSIX 字符串保存）。
    Remote { host_id: HostId, path: String },
}

impl WorkspaceLocation {
    pub fn kind(&self) -> WorkspaceKind {
        match self {
            WorkspaceLocation::Local { .. } => WorkspaceKind::Local,
            WorkspaceLocation::Remote { .. } => WorkspaceKind::Remote,
        }
    }
}

/// 用户保存的工作区。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedWorkspace {
    pub id: SavedWorkspaceId,
    /// 备注名。
    pub name: String,
    #[serde(flatten)]
    pub location: WorkspaceLocation,
    /// 最近一次通过 Wisp 打开的时间。
    #[serde(default)]
    pub last_opened_at: Option<DateTime<Utc>>,
}

impl SavedWorkspace {
    pub fn new(name: impl Into<String>, location: WorkspaceLocation) -> Self {
        Self {
            id: SavedWorkspaceId::new(),
            name: name.into(),
            location,
            last_opened_at: None,
        }
    }

    pub fn kind(&self) -> WorkspaceKind {
        self.location.kind()
    }

    pub fn host_id(&self) -> Option<HostId> {
        match &self.location {
            WorkspaceLocation::Local { .. } => None,
            WorkspaceLocation::Remote { host_id, .. } => Some(*host_id),
        }
    }

    pub fn local_path(&self) -> Option<&Path> {
        match &self.location {
            WorkspaceLocation::Local { path } => Some(path),
            WorkspaceLocation::Remote { .. } => None,
        }
    }
}

/// 比较两个本地路径是否指向同一位置（忽略末尾分隔符；Windows 上不区分大小写和分隔符）。
pub fn same_local_path(a: &Path, b: &Path) -> bool {
    fn normalize(path: &Path) -> String {
        let text = path.to_string_lossy();
        let text = if cfg!(windows) {
            text.replace('/', "\\").to_lowercase()
        } else {
            text.into_owned()
        };
        let trimmed = text.trim_end_matches(['/', '\\']);
        if trimmed.is_empty() {
            text
        } else {
            trimmed.to_string()
        }
    }
    normalize(a) == normalize(b)
}
