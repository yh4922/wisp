//! Wisp 界面国际化（i18n）。
//!
//! 采用 gettext 风格：**英文原文本身就是 key**。界面代码只需把字符串字面量包一层
//! [`t`]，当前语言的词典里找到译文就返回译文，否则原样返回英文。
//!
//! ```ignore
//! // 普通文案：返回 &'static str，可直接传给需要 SharedString / &str / String 的地方。
//! Button::new("open-folder", i18n::t("Open Folder"));
//! ContextMenu::build(window, cx, |menu, _, _| menu.action(i18n::t("Rename"), Box::new(Rename)));
//!
//! // 带参数的文案：key 中用 `{name}` 作为占位符，译文里保留同名占位符。
//! let label = i18n::tf!("Open {name}", name = file_name);
//! let label = i18n::t_args("Close {count} Items", &[("count", &count)]);
//! ```
//!
//! 词典位于 `assets/locales/<语言>.json`，格式为扁平的 `{"英文原文": "译文"}`，在编译期
//! 通过 `include_str!` 嵌入程序。英文没有单独的词典文件。新增文案时：
//! 1. 在代码中用 `i18n::t("English text")` 包裹；
//! 2. 在 `assets/locales/zh-CN.json` 中追加 `"English text": "中文译文"`。
//!
//! 当前语言保存在进程级全局变量中，由 [`set_language`] 切换；切换后界面在下一次重绘时
//! 即显示新语言（由 `zed` crate 监听 `ui_language` 设置并刷新窗口与原生菜单）。
//! 注意：调用 [`t`] 的结果若被缓存在 UI 状态里（例如输入框占位符在创建时设置），
//! 需要在重新创建该 UI 后才会更新。

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// 界面语言。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Language {
    /// 英文（原文，不查词典）。
    #[default]
    English = 0,
    /// 简体中文（zh-CN）。
    SimplifiedChinese = 1,
}

impl Language {
    /// 把 BCP 47 / POSIX 风格的语言标识（如 `zh-CN`、`zh_CN.UTF-8`、`en-US`）映射为界面语言。
    /// 以 `zh` 开头的映射为简体中文，其余为英文。
    pub fn from_locale(locale: &str) -> Self {
        if locale.trim().to_ascii_lowercase().starts_with("zh") {
            Language::SimplifiedChinese
        } else {
            Language::English
        }
    }

    /// 语言代码，例如 `en`、`zh-CN`。
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::SimplifiedChinese => "zh-CN",
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => Language::SimplifiedChinese,
            _ => Language::English,
        }
    }
}

static CURRENT_LANGUAGE: AtomicU8 = AtomicU8::new(Language::English as u8);
static ZH_CN: OnceLock<HashMap<String, String>> = OnceLock::new();

/// 设置当前界面语言。返回语言是否发生了变化。
pub fn set_language(language: Language) -> bool {
    CURRENT_LANGUAGE.swap(language as u8, Ordering::Relaxed) != language as u8
}

/// 当前界面语言。
pub fn language() -> Language {
    Language::from_u8(CURRENT_LANGUAGE.load(Ordering::Relaxed))
}

/// 检测操作系统的界面语言。
///
/// - Windows：`GetUserDefaultUILanguage`（主语言为中文时视为简体中文）；
/// - macOS / Linux：`sys-locale`（Linux 上依次读取 `LC_ALL`、`LC_MESSAGES`、`LANG`）。
pub fn system_language() -> Language {
    #[cfg(target_os = "windows")]
    {
        // LANGID 低 10 位是主语言 ID，LANG_CHINESE = 0x04。
        const LANG_CHINESE: u16 = 0x04;
        let lang_id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
        if lang_id & 0x3ff == LANG_CHINESE {
            Language::SimplifiedChinese
        } else {
            Language::English
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        sys_locale::get_locale()
            .map(|locale| Language::from_locale(&locale))
            .unwrap_or_default()
    }
}

fn dictionary(language: Language) -> Option<&'static HashMap<String, String>> {
    match language {
        Language::English => None,
        Language::SimplifiedChinese => {
            Some(ZH_CN.get_or_init(|| {
                parse_dictionary(include_str!("../../../assets/locales/zh-CN.json"))
            }))
        }
    }
}

fn parse_dictionary(json: &str) -> HashMap<String, String> {
    match serde_json::from_str::<HashMap<String, String>>(json) {
        Ok(mut map) => {
            map.retain(|_, value| !value.is_empty());
            map
        }
        Err(error) => {
            log::error!("failed to parse locale dictionary: {error}");
            HashMap::default()
        }
    }
}

/// 翻译一段界面文案。`key` 是英文原文；当前语言没有对应译文时原样返回。
pub fn t(key: &'static str) -> &'static str {
    lookup(language(), key).unwrap_or(key)
}

/// 翻译一段非 `'static` 的文案（例如运行时拼出来的 key），找不到译文时返回原文的拷贝。
pub fn t_dynamic(key: &str) -> String {
    lookup(language(), key)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| key.to_owned())
}

/// 在当前语言的词典中查找 `key` 的译文；没有译文（或当前语言为英文）时返回 `None`。
pub fn translate(key: &str) -> Option<&'static str> {
    lookup(language(), key)
}

fn lookup(language: Language, key: &str) -> Option<&'static str> {
    dictionary(language)?.get(key).map(String::as_str)
}

/// 翻译带占位符的文案，并把 `{name}` 替换为对应参数值。
///
/// ```ignore
/// i18n::t_args("Open {name}", &[("name", &file_name)]);
/// ```
pub fn t_args(key: &'static str, args: &[(&str, &dyn Display)]) -> String {
    let mut text = t(key).to_owned();
    for (name, value) in args {
        let placeholder = format!("{{{name}}}");
        if text.contains(&placeholder) {
            text = text.replace(&placeholder, &value.to_string());
        }
    }
    text
}

/// [`t_args`] 的便捷宏：`tf!("Open {name}", name = value)`。
#[macro_export]
macro_rules! tf {
    ($key:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::t_args(
            $key,
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),*],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translation_and_fallback() {
        set_language(Language::English);
        assert_eq!(t("Open Folder"), "Open Folder");

        set_language(Language::SimplifiedChinese);
        assert_eq!(t("Open Folder"), "打开文件夹");
        assert_eq!(
            t("Some text without translation"),
            "Some text without translation"
        );
        assert_eq!(tf!("Open {name}", name = "a.rs"), "打开 a.rs");
        set_language(Language::English);
        assert_eq!(t_args("Open {name}", &[("name", &"a.rs")]), "Open a.rs");
    }

    #[test]
    fn test_locale_mapping() {
        assert_eq!(Language::from_locale("zh-CN"), Language::SimplifiedChinese);
        assert_eq!(
            Language::from_locale("zh_TW.UTF-8"),
            Language::SimplifiedChinese
        );
        assert_eq!(Language::from_locale("en-US"), Language::English);
    }

    #[test]
    fn test_dictionary_placeholders_match() {
        // 译文可以省略原文中的占位符（例如中文不需要单复数），但不能引入原文没有的占位符。
        let dictionary = dictionary(Language::SimplifiedChinese).unwrap();
        assert!(!dictionary.is_empty());
        for (key, value) in dictionary {
            for placeholder in value.split('{').skip(1).filter_map(|s| s.split('}').next()) {
                assert!(
                    key.contains(&format!("{{{placeholder}}}")),
                    "translation of {key:?} uses unknown placeholder {{{placeholder}}}"
                );
            }
        }
    }
}
