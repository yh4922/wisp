//! 远程主机密码 / 私钥口令的读写，存放在系统凭据管理器中（Windows 为 Credential Manager）。
//!
//! 凭据 key 为 `wisp-host://<host_id>`，与 `workspaces.json` 中的主机通过 id 关联。

use anyhow::{Context as _, Result};
use gpui::{App, Task};

use crate::model::HostId;

/// 写入凭据时使用的用户名字段（真正的 SSH 用户名保存在 [`crate::Host::username`]）。
const SECRET_USERNAME: &str = "wisp";

/// 主机凭据的 key：`wisp-host://<host_id>`。
pub fn host_secret_url(host_id: HostId) -> String {
    format!("wisp-host://{host_id}")
}

/// 保存主机的密码或私钥口令（覆盖已有值）。
pub fn save_host_secret(host_id: HostId, secret: String, cx: &App) -> Task<Result<()>> {
    let provider = zed_credentials_provider::global(cx);
    let url = host_secret_url(host_id);
    cx.spawn(async move |cx| {
        provider
            .write_credentials(&url, SECRET_USERNAME, secret.as_bytes(), cx)
            .await
            .with_context(|| format!("saving credentials for {url}"))
    })
}

/// 读取主机的密码或私钥口令；没有保存过时返回 `None`。
pub fn load_host_secret(host_id: HostId, cx: &App) -> Task<Result<Option<String>>> {
    let provider = zed_credentials_provider::global(cx);
    let url = host_secret_url(host_id);
    cx.spawn(async move |cx| {
        let Some((_, secret)) = provider
            .read_credentials(&url, cx)
            .await
            .with_context(|| format!("reading credentials for {url}"))?
        else {
            return Ok(None);
        };
        let secret = String::from_utf8(secret).context("credentials are not valid UTF-8")?;
        Ok(Some(secret))
    })
}

/// 删除主机的凭据。
pub fn delete_host_secret(host_id: HostId, cx: &App) -> Task<Result<()>> {
    let provider = zed_credentials_provider::global(cx);
    let url = host_secret_url(host_id);
    cx.spawn(async move |cx| {
        provider
            .delete_credentials(&url, cx)
            .await
            .with_context(|| format!("deleting credentials for {url}"))
    })
}
