# Wisp 开发任务

需求见 `.docs/Wisp/需求文档.md`。

## M0 初始化
- [x] 浅克隆 Zed v1.29.3
- [x] SVG Logo（彩色 + 单色）
- [x] 需求文档
- [ ] 独立初始提交，推送 github.com/yh4922/wisp

## M1 功能删减（方案见 .docs/Wisp/M1删减方案.md）
- [x] 梳理依赖图
- [x] 第一批：叶子 crate（repl、vim、collab_ui、debugger_ui、onboarding 等 19 个）
- [x] 第二批：AI 前端（37 个 crate）
- [x] 第三批：AI 基础（agent_settings、language_model、prompt_store、context_server 等 8 个）
- [x] 第四批：扩展系统（extension、extension_host 等 6 个 + extensions/）
- [x] 第五批：协作/云/遥测/自动更新（call、channel、livekit、auto_update、crashes 等 11 个）；client 精简为本地实现，telemetry 空实现
- [x] remote server 改从 github.com/yh4922/wisp Release 下载
- [x] `cargo build` 通过并启动验证
- 保留：edit_prediction_types（editor 深度耦合，无提供者，不生效）、dap / project::debugger（无 UI）

## M2 品牌
- [x] 可执行文件 wisp.exe、wisp-remote-server，版本 0.1.0
- [x] 单实例标识 Wisp-Editor-*、配置目录 %APPDATA%\Wisp、项目目录 .wisp、URL scheme wisp://
- [x] 用户可见文案 Zed → Wisp，关于页注明 Based on Zed，欢迎页 Logo
- [x] 生成 ico / png 图标（script/gen-icons.ps1）
- [x] 修复：title_bar::init 随 collab_ui 丢失；默认窗口超出工作区

## 环境备注
- 编译前执行 `. .\script\wisp-env.ps1`（Rust 1.98.1、MSVC、cmake、Spectre 绕过、关闭增量编译）
- 本机 `cargo test` 会触发 rustc 崩溃（STATUS_STACK_BUFFER_OVERRUN），测试只做 `cargo check --tests`

## M3 i18n
- [x] crates/i18n：t() / tf! / t_args，英文原文为 key，assets/locales/zh-CN.json（约 700 条）
- [x] 设置 ui_language（system / zh-CN / en），设置页「界面语言」下拉，切换即时生效并重建菜单
- [x] 覆盖：原生菜单、标题栏、欢迎页、信任对话框、项目面板 / 编辑器 / 标签页 / 终端右键菜单、Git 面板、搜索、设置页常用项、保存提示

## M4 工作区（设计见 .docs/Wisp/工作区设计.md）
- [x] wisp_workspaces：数据模型、workspaces.json 原子读写、WorkspaceStore、主机凭据 API
- [x] workspace_panel：左侧「工作区」面板（ctrl-alt-k / cmd-alt-k）、命名弹窗、启动页工作区区块
- [x] project_panel 默认停靠左侧

## M5 SSH 远程
- [x] 打开远程对话框：主机列表 → 主机表单（密码 / 私钥 + 口令、测试连接）→ 远程目录浏览 → 命名打开
- [x] 密码存 Windows 凭据管理器（wisp-host://<id>），连接时注入 SshConnectionOptions.password，失败回退密码框
- [x] 远程服务端缺失时中文提示
- [ ] 真实服务器端到端验收（需用户）

## Q15 内置语言
- [x] HTML、Vue、Java、PHP、Dockerfile、SQL、TOML、Nginx（crates/grammars），查询编译测试通过
- [ ] 各语言 LSP 接入（未做）

## M6 CI 发布
- [x] .github/workflows/release.yml（tag v* → Linux remote server x86_64/aarch64 + Windows 安装包/便携版 → Release）
- [x] .github/workflows/ci.yml
- [x] crates/zed/resources/windows/wisp.iss（新 AppId、wisp://、用 Wisp 打开）、script/bundle-windows.ps1
- [ ] 首次在 GitHub 上实际运行

## 审查记录
- 验证：每个里程碑 `cargo build -p zed` 无 error / warning；启动截图见 .tasks/shots/
- 本机 cargo test 不可用（rustc 崩溃），仅 languages crate 单测实际运行通过；其余只验证可编译
- 剩余风险：
  1. SSH 远程从未连过真实服务器；Release 未发布前连接必然提示「未找到远程服务端」
  2. CI workflow 未在 GitHub 跑过，Windows release 构建（codegen-units=1）可能耗时 1–2 小时
  3. 多个 crate 的测试仍断言 .zed / Zed 文案，运行时可能失败
  4. 保留 edit_prediction_types、dap、client（精简）等休眠代码
  5. tailwind / eslint LSP 会挂到 HTML / Vue / PHP 上自动启动
  6. 安装包未代码签名（SmartScreen 会提示）
