//! `workspaces.json` 的结构与读写。
//!
//! - 文件不存在（或为空）时视为空列表；
//! - 解析失败时记录日志，并把原文件改名为 `workspaces.json.bak` 后视为空列表，绝不 panic；
//! - 写入时先写同目录下的临时文件，再 rename 覆盖目标文件，避免写一半导致文件损坏。

use std::cmp::Reverse;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::model::{Host, HostId, SavedWorkspace, SavedWorkspaceId, WorkspaceLocation};

/// 当前文件格式版本。
pub const CURRENT_VERSION: u32 = 1;

/// 默认数据文件路径：`<config_dir>/workspaces.json`。
pub fn workspaces_file_path() -> PathBuf {
    paths::config_dir().join("workspaces.json")
}

fn current_version() -> u32 {
    CURRENT_VERSION
}

/// `workspaces.json` 的完整内容。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspacesFile {
    #[serde(default = "current_version")]
    pub version: u32,
    #[serde(default)]
    pub hosts: Vec<Host>,
    #[serde(default)]
    pub workspaces: Vec<SavedWorkspace>,
}

impl Default for WorkspacesFile {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            hosts: Vec::new(),
            workspaces: Vec::new(),
        }
    }
}

impl WorkspacesFile {
    pub fn host(&self, id: HostId) -> Option<&Host> {
        self.hosts.iter().find(|host| host.id == id)
    }

    pub fn find_host(&self, host: &str, port: u16, username: &str) -> Option<&Host> {
        self.hosts.iter().find(|candidate| {
            candidate.host.eq_ignore_ascii_case(host)
                && candidate.port == port
                && candidate.username == username
        })
    }

    pub fn workspace(&self, id: SavedWorkspaceId) -> Option<&SavedWorkspace> {
        self.workspaces.iter().find(|workspace| workspace.id == id)
    }

    pub fn workspace_mut(&mut self, id: SavedWorkspaceId) -> Option<&mut SavedWorkspace> {
        self.workspaces
            .iter_mut()
            .find(|workspace| workspace.id == id)
    }

    /// 按 `last_opened_at` 倒序；从未打开过的排在最后，同组内按名称排序。
    pub fn workspaces_by_last_opened(&self) -> Vec<&SavedWorkspace> {
        let mut workspaces: Vec<_> = self.workspaces.iter().collect();
        workspaces.sort_by(|a, b| {
            Reverse(a.last_opened_at)
                .cmp(&Reverse(b.last_opened_at))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        workspaces
    }

    pub fn find_local_workspace(&self, path: &Path) -> Option<&SavedWorkspace> {
        self.workspaces.iter().find(|workspace| {
            workspace
                .local_path()
                .is_some_and(|saved| crate::model::same_local_path(saved, path))
        })
    }

    pub fn find_remote_workspace(&self, host_id: HostId, path: &str) -> Option<&SavedWorkspace> {
        let path = path.trim_end_matches('/');
        self.workspaces
            .iter()
            .find(|workspace| match &workspace.location {
                WorkspaceLocation::Remote {
                    host_id: saved_host,
                    path: saved_path,
                } => *saved_host == host_id && saved_path.trim_end_matches('/') == path,
                WorkspaceLocation::Local { .. } => false,
            })
    }
}

/// 读取数据文件。任何错误都不会向上传播：不存在视为空，损坏则备份后视为空。
pub fn load_file(path: &Path) -> WorkspacesFile {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return WorkspacesFile::default(),
        Err(error) => {
            log::error!("failed to read {}: {error}", path.display());
            return WorkspacesFile::default();
        }
    };
    if text.trim().is_empty() {
        return WorkspacesFile::default();
    }

    match serde_json::from_str::<WorkspacesFile>(&text) {
        Ok(file) => {
            if file.version > CURRENT_VERSION {
                log::warn!(
                    "{} has version {}, newer than supported version {CURRENT_VERSION}",
                    path.display(),
                    file.version
                );
            }
            file
        }
        Err(error) => {
            let backup = backup_path(path);
            log::error!(
                "failed to parse {}: {error}; moving it to {}",
                path.display(),
                backup.display()
            );
            if let Err(error) = std::fs::rename(path, &backup) {
                log::error!("failed to back up {}: {error}", path.display());
            }
            WorkspacesFile::default()
        }
    }
}

/// 原子写入数据文件（先写临时文件，再 rename 覆盖）。
pub fn save_file(path: &Path, file: &WorkspacesFile) -> Result<()> {
    let json = serde_json::to_string_pretty(file).context("serializing workspaces")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    let temp_path = temp_path(path);
    std::fs::write(&temp_path, json.as_bytes())
        .with_context(|| format!("writing {}", temp_path.display()))?;
    if let Err(error) = std::fs::rename(&temp_path, path) {
        std::fs::remove_file(&temp_path).ok();
        return Err(error).with_context(|| format!("replacing {}", path.display()));
    }
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "workspaces.json".into());
    name.push(suffix);
    path.with_file_name(name)
}

/// 解析失败时的备份路径：`workspaces.json.bak`。
pub fn backup_path(path: &Path) -> PathBuf {
    with_suffix(path, ".bak")
}

fn temp_path(path: &Path) -> PathBuf {
    with_suffix(path, ".tmp")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AuthMethod, WorkspaceKind};
    use chrono::{TimeZone as _, Utc};

    #[test]
    fn test_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = load_file(&dir.path().join("workspaces.json"));
        assert_eq!(file, WorkspacesFile::default());
    }

    #[test]
    fn test_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("workspaces.json");

        let host = Host::new(
            "测试服务器",
            "192.168.1.10",
            22,
            "root",
            AuthMethod::Password,
            None,
        );
        let host_id = host.id;
        let mut file = WorkspacesFile::default();
        file.hosts.push(host);
        file.workspaces.push(SavedWorkspace::new(
            "我的项目",
            WorkspaceLocation::Local {
                path: PathBuf::from("D:\\Project\\Foo"),
            },
        ));
        file.workspaces.push(SavedWorkspace::new(
            "线上服务",
            WorkspaceLocation::Remote {
                host_id,
                path: "/srv/app".into(),
            },
        ));

        save_file(&path, &file).unwrap();
        assert!(!temp_path(&path).exists());
        let loaded = load_file(&path);
        assert_eq!(loaded, file);
        assert_eq!(loaded.workspaces[0].kind(), WorkspaceKind::Local);
        assert_eq!(loaded.workspaces[1].host_id(), Some(host_id));
    }

    #[test]
    fn test_parses_documented_format() {
        let json = r#"{
            "version": 1,
            "hosts": [{
                "id": "6f1c1d1e-6d3a-4a51-9a51-0c2f0b8f2a11",
                "name": "测试服务器",
                "host": "192.168.1.10",
                "port": 22,
                "username": "root",
                "auth": "password",
                "key_path": null
            }],
            "workspaces": [
                { "id": "0b7d2b0c-1f84-4c58-8f5e-6f3e4b9d3a01", "name": "我的项目", "kind": "local", "path": "D:\\Project\\Foo", "last_opened_at": "2026-11-24T00:00:00Z" },
                { "id": "0b7d2b0c-1f84-4c58-8f5e-6f3e4b9d3a02", "name": "线上服务", "kind": "remote", "host_id": "6f1c1d1e-6d3a-4a51-9a51-0c2f0b8f2a11", "path": "/srv/app", "last_opened_at": null }
            ]
        }"#;
        let file: WorkspacesFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.hosts.len(), 1);
        assert_eq!(file.hosts[0].auth, AuthMethod::Password);
        assert_eq!(
            file.workspaces[0].last_opened_at,
            Some(Utc.with_ymd_and_hms(2026, 11, 24, 0, 0, 0).unwrap())
        );
        assert_eq!(file.workspaces[1].host_id(), Some(file.hosts[0].id));

        let value = serde_json::to_value(&file.workspaces[0]).unwrap();
        assert_eq!(value["kind"], "local");
    }

    #[test]
    fn test_corrupt_file_is_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspaces.json");
        std::fs::write(&path, "{ not json").unwrap();

        let file = load_file(&path);
        assert_eq!(file, WorkspacesFile::default());
        assert!(!path.exists());
        assert_eq!(
            std::fs::read_to_string(backup_path(&path)).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn test_sorted_by_last_opened() {
        let mut file = WorkspacesFile::default();
        let mut older = SavedWorkspace::new("b", WorkspaceLocation::Local { path: "/b".into() });
        older.last_opened_at = Some(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap());
        let mut newer = SavedWorkspace::new("c", WorkspaceLocation::Local { path: "/c".into() });
        newer.last_opened_at = Some(Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap());
        let never = SavedWorkspace::new("a", WorkspaceLocation::Local { path: "/a".into() });
        file.workspaces = vec![never, older, newer];

        let names: Vec<_> = file
            .workspaces_by_last_opened()
            .into_iter()
            .map(|workspace| workspace.name.as_str())
            .collect();
        assert_eq!(names, ["c", "b", "a"]);
        assert!(file.find_local_workspace(Path::new("/b/")).is_some());
    }
}
