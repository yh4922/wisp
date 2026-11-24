<p align="center">
  <img src="assets/branding/png/wisp-256.png" width="128" alt="Wisp">
</p>

<h1 align="center">Wisp</h1>

<p align="center">轻量、快速的代码编辑器 —— 只保留编辑器、终端与 SSH 远程。</p>

---

Wisp 是 [Zed](https://github.com/zed-industries/zed)（基于 v1.29.3）的硬分叉版本：

- 去除 AI 助手、插件市场、云账号登录与多人协作、遥测、自动更新
- 保留编辑器、LSP、Tree-sitter、项目文件树、全局搜索、Git、终端
- 新增工作区面板：一键打开本地文件夹或保存的 SSH 远程工作区
- 界面支持简体中文 / English

## 下载

预编译版本发布在 [GitHub Releases](https://github.com/yh4922/wisp/releases) 页面：

- `Wisp-<版本>-x86_64-setup.exe`：Windows 安装包（安装到 `%LOCALAPPDATA%\Programs\Wisp`，无需管理员权限）
- `Wisp-<版本>-x86_64-portable.zip`：Windows 便携版，解压后运行 `Wisp\wisp.exe`
- `wisp-remote-server-linux-<架构>.gz`：SSH 远程开发的服务端，Wisp 连接远程主机时会自动下载，一般无需手动获取
- `SHA256SUMS.txt`：校验和

## 构建

```sh
cargo run --release
```

Windows 构建环境要求参考 [docs/src/development/windows.md](docs/src/development/windows.md)。

## 许可

Wisp 继承 Zed 的许可：编辑器部分为 GPL-3.0，GPUI 等库为 Apache-2.0，详见各 crate 下的 LICENSE 文件。

Zed 及其商标归 Zed Industries, Inc. 所有，Wisp 与 Zed Industries 无隶属关系。
