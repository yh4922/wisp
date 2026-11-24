//! 轻量 SSH 命令执行：「测试连接」和远程目录浏览使用。
//!
//! Zed 的连接层（`remote::connect`）在建立连接时一定会检查并上传远程服务端，无法只做
//! 认证或列目录，因此这里直接调用系统 `ssh`（Windows 自带 OpenSSH）执行一条命令，
//! 密码 / 私钥口令通过 Zed 的 askpass 机制（[`askpass::PasswordProxy`]，与 `remote`
//! 传输层在 Windows 上复用连接时的做法相同）自动应答，不弹任何界面。

use std::{
    ops::ControlFlow,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context as _, Result};
use askpass::EncryptedPassword;
use futures::{FutureExt as _, select_biased};
use gpui::{AsyncApp, Task};
use wisp_workspaces::{AuthMethod, Host};

/// 整条命令（含连接、认证）的超时。
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
/// TCP 连接超时（秒）。
const CONNECT_TIMEOUT_SECS: u32 = 10;

/// 执行 SSH 命令所需的连接参数。
#[derive(Clone, Debug)]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    pub key_path: Option<PathBuf>,
    /// 密码（密码认证）或私钥口令（私钥认证），可为空。
    pub secret: Option<String>,
}

impl SshTarget {
    pub fn from_host(host: &Host, secret: Option<String>) -> Self {
        Self {
            host: host.host.clone(),
            port: host.port,
            username: host.username.clone(),
            auth: host.auth,
            key_path: host.key_path.clone(),
            secret,
        }
    }

    fn ssh_args(&self) -> Vec<String> {
        let mut args: Vec<String> = [
            "-T",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "NumberOfPasswordPrompts=1",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        args.push("-o".into());
        args.push(format!("ConnectTimeout={CONNECT_TIMEOUT_SECS}"));
        args.push("-p".into());
        args.push(self.port.to_string());
        match self.auth {
            AuthMethod::Key => {
                if let Some(key_path) = &self.key_path {
                    args.push("-i".into());
                    args.push(key_path.to_string_lossy().into_owned());
                }
                args.extend(
                    [
                        "-o",
                        "IdentitiesOnly=yes",
                        "-o",
                        "PreferredAuthentications=publickey",
                    ]
                    .map(String::from),
                );
            }
            AuthMethod::Password => {
                args.extend(
                    [
                        "-o",
                        "PreferredAuthentications=password,keyboard-interactive",
                    ]
                    .map(String::from),
                );
            }
        }
        if !self.username.is_empty() {
            args.push("-l".into());
            args.push(self.username.clone());
        }
        args.push("--".into());
        args.push(self.host.clone());
        args
    }
}

/// 应答 ssh 的 askpass 提示：密码 / 口令只回答一次（答错时 ssh 会再问，此时回答空串让它失败，
/// 而不是无限重试）；其他提示一律回答空串。
fn answer_for(target: &SshTarget, prompt: &str, answered: &AtomicBool) -> String {
    let prompt = prompt.to_lowercase();
    let wanted = match target.auth {
        AuthMethod::Password => prompt.contains("password"),
        AuthMethod::Key => prompt.contains("passphrase"),
    };
    if wanted && !answered.swap(true, Ordering::SeqCst) {
        target.secret.clone().unwrap_or_default()
    } else {
        String::new()
    }
}

/// 用 `target` 连接并执行 `remote_command`（交给远程登录 shell 解释），返回标准输出。
/// 退出码非 0 时返回包含 ssh 标准错误输出的错误。
pub async fn run(target: SshTarget, remote_command: String, cx: &mut AsyncApp) -> Result<String> {
    let answered = Arc::new(AtomicBool::new(false));
    let answer_target = target.clone();
    let get_password = move |prompt: String| {
        let answer = answer_for(&answer_target, &prompt, &answered);
        Task::ready(ControlFlow::Continue(EncryptedPassword::try_from(
            answer.as_str(),
        )))
    };
    let executor = cx.background_executor().clone();
    let proxy = askpass::PasswordProxy::new(Box::new(get_password), executor.clone())
        .await
        .context("starting askpass")?;

    let mut command = util::command::new_command("ssh");
    command
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("SSH_ASKPASS", proxy.script_path());
    #[cfg(windows)]
    command.env("ZED_ASKPASS_SOCKET", proxy.socket_path());
    command.args(target.ssh_args()).arg(remote_command);
    log::debug!("wisp ssh: {:?}", target.ssh_args());

    let output = select_biased! {
        output = command.output().fuse() => output.context(i18n::t("Failed to run ssh. Please make sure OpenSSH is installed."))?,
        _ = executor.timer(COMMAND_TIMEOUT).fuse() => {
            anyhow::bail!(i18n::t("Connection timed out"));
        }
    };
    drop(proxy);

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        return Ok(stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("Warning: Permanently added"))
        .collect::<Vec<_>>()
        .join("\n");
    if message.is_empty() {
        anyhow::bail!(i18n::tf!(
            "ssh exited with code {code}",
            code = output.status.code().unwrap_or(-1)
        ));
    }
    anyhow::bail!(message)
}

/// 测试连接：执行 `echo ok`。
pub async fn test_connection(target: SshTarget, cx: &mut AsyncApp) -> Result<()> {
    let output = run(target, "echo wisp-ok".to_string(), cx).await?;
    anyhow::ensure!(
        output.contains("wisp-ok"),
        i18n::t("Unexpected response from the remote host")
    );
    Ok(())
}

/// 远程目录列表。
#[derive(Clone, Debug, Default)]
pub struct RemoteDirListing {
    /// 解析后的绝对路径。
    pub path: String,
    /// 子目录名（不含 `.` / `..`），已排序，隐藏目录排在最后。
    pub dirs: Vec<String>,
}

/// POSIX shell 单引号转义。
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// 列出 `path` 下的子目录。`path` 支持 `~` / `~/...`（展开为远程用户的 home），空串视为 `~`。
pub async fn list_dirs(target: SshTarget, path: String, cx: &mut AsyncApp) -> Result<RemoteDirListing> {
    // 脚本里不能出现单引号（整体放在单引号里交给 sh -c），路径作为 $1 传入。
    const SCRIPT: &str = r#"p="$1"; case "$p" in ""|"~") p="$HOME";; "~/"*) p="$HOME/${p#"~/"}";; esac; cd -- "$p" || exit 3; pwd; LC_ALL=C ls -1paL 2>/dev/null; exit 0"#;
    let remote_command = format!(
        "sh -c {} wisp-ls {}",
        shell_quote(SCRIPT),
        shell_quote(path.trim())
    );
    let output = run(target, remote_command, cx).await?;
    Ok(parse_listing(&output))
}

fn parse_listing(output: &str) -> RemoteDirListing {
    let mut lines = output.lines().map(|line| line.trim_end_matches('\r'));
    let path = lines.next().unwrap_or("/").trim().to_string();
    let mut dirs: Vec<String> = lines
        .filter_map(|line| line.strip_suffix('/'))
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .map(String::from)
        .collect();
    dirs.sort_by(|a, b| {
        (a.starts_with('.'), a.to_lowercase()).cmp(&(b.starts_with('.'), b.to_lowercase()))
    });
    dirs.dedup();
    RemoteDirListing { path, dirs }
}

/// 拼接远程路径（POSIX）。
pub fn join_remote_path(parent: &str, child: &str) -> String {
    if child == ".." {
        return parent_remote_path(parent);
    }
    if parent.ends_with('/') {
        format!("{parent}{child}")
    } else {
        format!("{parent}/{child}")
    }
}

/// 上级目录；根目录的上级仍是根目录。
pub fn parent_remote_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(index) => trimmed[..index].to_string(),
    }
}

/// 远程目录名（用作工作区默认名称）。
pub fn remote_dir_name(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("/")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_listing() {
        let listing = parse_listing("/home/me\n./\n../\n.cache/\nsrc/\nREADME.md\nApp/\nlink\n");
        assert_eq!(listing.path, "/home/me");
        assert_eq!(listing.dirs, vec!["App", "src", ".cache"]);
    }

    #[test]
    fn test_remote_paths() {
        assert_eq!(join_remote_path("/", "srv"), "/srv");
        assert_eq!(join_remote_path("/srv", "app"), "/srv/app");
        assert_eq!(join_remote_path("/srv/app", ".."), "/srv");
        assert_eq!(parent_remote_path("/srv"), "/");
        assert_eq!(parent_remote_path("/"), "/");
        assert_eq!(remote_dir_name("/srv/app/"), "app");
        assert_eq!(remote_dir_name("/"), "/");
        assert_eq!(shell_quote("a'b"), r"'a'\''b'");
    }
}
