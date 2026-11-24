# M3 i18n 子任务通用说明（给子 agent）

仓库：D:\Project\Wisp（Zed v1.29.3 硬分叉，改名 Wisp），Windows PowerShell 5.1。请用中文回复。

## 已就绪的基础设施（不要修改）
- crate `crates/i18n`（workspace 依赖 `i18n`）。API：
  - `i18n::t(key: &'static str) -> &'static str`：英文原文作 key，查当前语言词典，查不到返回原文。
    返回 `&'static str`，可直接传给 `impl Into<SharedString>`、`&str`、`.to_string()` 等。
  - `i18n::t_dynamic(key: &str) -> String`：key 不是 'static 时使用。
  - `i18n::tf!("Open {name}", name = value)` / `i18n::t_args("Open {name}", &[("name", &value)])` -> String：带占位符。
- 词典：`assets/locales/zh-CN.json`（扁平 {"英文": "中文"}）。**子 agent 不要直接编辑它**，
  而是把你新增的条目写到 `.tasks/i18n/<你的名字>.json`（同样扁平 JSON 对象，UTF-8），由 lead 合并。
- 设置项 `ui_language`（settings::UiLanguage：System/ZhCn/En）已加到 settings_content。

## 做法
- 每处只把字符串字面量包一层 `i18n::t(...)`，尽量少改结构；注意类型/生命周期
  （`SharedString`、`&'static str`、`String`）。如果字符串是 `const`/`static` 中的，改在使用处调用 `t`。
- format!("...{}...", x) 形式的界面文案改用 `i18n::tf!` 并在 JSON 中保留同名占位符。
- 给对应 crate 的 Cargo.toml `[dependencies]` 按字母序加 `i18n.workspace = true`。
- 不翻译：action/命令名、keybinding 上下文、telemetry/日志、ID、测试中断言的字符串（若测试断言了被翻译的英文，
  英文模式下仍返回原文，所以测试不受影响，无需改测试）。
- 翻译要地道，用 VS Code 简体中文版的习惯用语：Explorer→资源管理器，Command Palette→命令面板，
  Open Folder→打开文件夹，Settings→设置，Keymap→键盘快捷方式，Reveal in File Explorer→在文件资源管理器中显示，
  Copy Path→复制路径，Copy Relative Path→复制相对路径，Rename→重命名，Delete→删除，Cut→剪切，Copy→复制，Paste→粘贴，
  Stage→暂存，Unstage→取消暂存，Commit→提交，Push→推送，Pull→拉取，Fetch→抓取，Discard→放弃更改，
  Outline→大纲，Diagnostics→诊断，Terminal→终端，Source Control/Git Panel→源代码管理/Git 面板，Find→查找，Replace→替换，
  Match Case→区分大小写，Match Whole Word→全字匹配，Use Regular Expression→使用正则表达式，Close→关闭，
  Close Others→关闭其他，Close to the Right→关闭右侧，Close All→全部关闭，Pin Tab→固定标签页，Split Right→向右拆分。
- 遵循 rustfmt 风格；不 commit/push；不重构无关代码；不运行 cargo test。

## 编译检查
在同一条 PowerShell 命令里先加载环境（可能要等其他 agent 的 cargo 锁，给足 timeout 例如 1500000ms）：
`. .\script\wisp-env.ps1; cargo check -p <crate> --message-format short 2>&1 | Where-Object { $_ -match '^(error|warning)|^\s+-->|Finished|could not compile' } | Where-Object { $_ -notmatch 'linker stdout|generated 1 warning' } | Select-Object -First 80`
必须无 error、无 warning（只看你改动的 crate）。

## 交付
最终回复中列出：改动的文件、新增词条数、`.tasks/i18n/<名字>.json` 路径、编译结果、遗留问题。
