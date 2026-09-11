//! 界面语言（需求 F-9.1）。引擎、队列与校验产出的说明文字按设置里的语言生成，中文为主、英文备选。
//!
//! 写法：`tr!(lang, "中文 {}", "English {}", arg)`。参数按位置共用，两种语言都要引用到；
//! 需要调换顺序时用 `{0}` `{1}`。宏里的格式串不能隐式捕获变量，参数一律显式传入。

pub use crate::config::Lang;

/// 按语言二选一
pub fn pick<'a>(lang: Lang, zh: &'a str, en: &'a str) -> &'a str {
    match lang {
        Lang::En => en,
        Lang::ZhCn => zh,
    }
}

#[macro_export]
macro_rules! tr {
    ($lang:expr, $zh:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        match $lang {
            $crate::config::Lang::En => format!($en $(, $arg)*),
            $crate::config::Lang::ZhCn => format!($zh $(, $arg)*),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_by_language_with_shared_arguments() {
        let n = 3;
        assert_eq!(tr!(Lang::ZhCn, "{} 条音轨", "{} audio tracks", n), "3 条音轨");
        assert_eq!(tr!(Lang::En, "{} 条音轨", "{} audio tracks", n), "3 audio tracks");
        assert_eq!(tr!(Lang::En, "{0} 到 {1}", "from {0} to {1}", "a", "b"), "from a to b");
        assert_eq!(pick(Lang::En, "完成", "Done"), "Done");
    }
}
