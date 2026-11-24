//! Downloads Wisp remote server binaries from GitHub Releases.
//!
//! Release assets are published as
//! `https://github.com/yh4922/wisp/releases/download/v{version}/wisp-remote-server-{os}-{arch}.gz`
//! where `os` is `linux` or `macos` and `arch` is `x86_64` or `aarch64`.
//!
//! Downloaded archives are cached under [`paths::remote_servers_dir`] so that an
//! archive which is already present locally is never downloaded again.

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context as _, Result};
use futures::StreamExt as _;
use gpui::AsyncApp;
use http_client::AsyncBody;
use release_channel::{AppVersion, ReleaseChannel};
use remote::{RemoteOs, RemotePlatform};
use semver::Version;

const RELEASES_BASE_URL: &str = "https://github.com/yh4922/wisp/releases/download";

/// How many cached archives to keep per platform.
const REMOTE_SERVER_CACHE_LIMIT: usize = 5;

/// Returns the version to download: the requested one, or the running app's
/// version, with pre-release and build metadata stripped.
fn release_version(version: Option<Version>, cx: &AsyncApp) -> Version {
    let mut version = version.unwrap_or_else(|| cx.update(|cx| AppVersion::global(cx)));
    version.pre = semver::Prerelease::EMPTY;
    version.build = semver::BuildMetadata::EMPTY;
    version
}

/// Returns the GitHub Releases URL of the remote server archive for `platform`.
pub fn remote_server_release_url(platform: RemotePlatform, version: &Version) -> Result<String> {
    anyhow::ensure!(
        !matches!(platform.os, RemoteOs::Windows),
        i18n::t("Wisp doesn't publish remote server releases for Windows hosts")
    );
    Ok(format!(
        "{RELEASES_BASE_URL}/v{version}/wisp-remote-server-{}-{}.gz",
        platform.os.as_str(),
        platform.arch.as_str(),
    ))
}

/// Returns the URL the remote host should download the server archive from.
pub fn get_remote_server_release_url(
    platform: RemotePlatform,
    version: Option<Version>,
    cx: &AsyncApp,
) -> Result<Option<String>> {
    let version = release_version(version, cx);
    remote_server_release_url(platform, &version).map(Some)
}

/// Downloads the remote server archive for `platform` into the local cache
/// (unless it is already cached) and returns its path.
pub async fn download_remote_server_release(
    platform: RemotePlatform,
    release_channel: ReleaseChannel,
    version: Option<Version>,
    set_status: impl Fn(&str, &mut AsyncApp),
    cx: &mut AsyncApp,
) -> Result<PathBuf> {
    let version = release_version(version, cx);
    let url = remote_server_release_url(platform, &version)?;

    let platform_dir = paths::remote_servers_dir()
        .join(release_channel.dev_name())
        .join(format!("{}-{}", platform.os.as_str(), platform.arch.as_str()));
    let version_path = platform_dir.join(format!("{version}.gz"));
    smol::fs::create_dir_all(&platform_dir).await.ok();

    if smol::fs::metadata(&version_path).await.is_err() {
        log::info!(
            "downloading wisp-remote-server {} {} version {version}",
            platform.os,
            platform.arch
        );
        set_status("Downloading remote server", cx);
        let http_client = cx.update(|cx| cx.http_client());
        download_file(&url, &version, &version_path, http_client.as_ref()).await?;
    }

    if let Err(error) =
        cleanup_remote_server_cache(&platform_dir, &version_path, REMOTE_SERVER_CACHE_LIMIT).await
    {
        log::warn!("Failed to clean up remote server cache in {platform_dir:?}: {error:#}");
    }

    Ok(version_path)
}

async fn download_file(
    url: &str,
    version: &Version,
    target_path: &Path,
    http_client: &dyn http_client::HttpClient,
) -> Result<()> {
    let mut response = http_client
        .get(url, AsyncBody::default(), true)
        .await
        .with_context(|| i18n::tf!("Unable to download the remote server from {url}", url = url))?;
    let status = response.status();
    // 还没有发布对应版本的 Release 时 GitHub 返回 404，给出明确的提示。
    anyhow::ensure!(
        status != http_client::StatusCode::NOT_FOUND,
        i18n::tf!(
            "Remote server v{version} was not found. Please make sure the v{version} release has been published ({url}).",
            version = version,
            url = url
        )
    );
    anyhow::ensure!(
        status.is_success(),
        i18n::tf!(
            "Failed to download remote server v{version} from {url} (HTTP {status}).",
            version = version,
            url = url,
            status = status.as_u16()
        )
    );

    let temp_path = target_path.with_extension(format!("download-{}", std::process::id()));
    let result = async {
        let mut temp_file = smol::fs::File::create(&temp_path).await?;
        smol::io::copy(response.body_mut(), &mut temp_file).await?;
        drop(temp_file);
        smol::fs::rename(&temp_path, target_path).await?;
        anyhow::Ok(())
    }
    .await;
    if result.is_err() {
        smol::fs::remove_file(&temp_path).await.ok();
    }
    result
}

async fn cleanup_remote_server_cache(
    platform_dir: &Path,
    keep_path: &Path,
    limit: usize,
) -> Result<()> {
    if limit == 0 {
        return Ok(());
    }

    let mut entries = smol::fs::read_dir(platform_dir).await?;
    let now = SystemTime::now();
    let mut candidates = Vec::new();

    while let Some(entry) = entries.next().await {
        let entry = entry?;
        let path = entry.path();
        if path.extension() != Some(OsStr::new("gz")) {
            continue;
        }

        let mtime = if path == keep_path {
            now
        } else {
            smol::fs::metadata(&path)
                .await
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        };

        candidates.push((path, mtime));
    }

    if candidates.len() <= limit {
        return Ok(());
    }

    candidates.sort_by(|(path_a, time_a), (path_b, time_b)| {
        time_b.cmp(time_a).then_with(|| path_a.cmp(path_b))
    });

    for (index, (path, _)) in candidates.into_iter().enumerate() {
        if index < limit || path == keep_path {
            continue;
        }

        if let Err(error) = smol::fs::remove_file(&path).await {
            log::warn!("Failed to remove old remote server archive {path:?}: {error}");
        }
    }

    Ok(())
}
