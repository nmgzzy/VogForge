//! ffmpeg / ffprobe 报错转成用户看得懂的说明（需求 F-9.4）：原因加可行动作，原文单独保留供展开查看。
//!
//! 判据是从真实报错里挑的 stderr 子串（不区分大小写），按表的顺序取第一个命中的。硬件编码失败的分类
//! 与回退在 `classify.rs`，这里只负责"最后还是失败了"时怎么向用户解释。

use crate::i18n::Lang;

use super::classify::key_line;

/// 一条解释：原因、建议的动作、ffmpeg 原文（最能说明问题的那一行）
#[derive(Debug, Clone, PartialEq)]
pub struct Explained {
    pub summary: String,
    pub action: String,
    pub raw: String,
}

impl Explained {
    /// 合成一句给界面的话
    pub fn sentence(&self, lang: Lang) -> String {
        match lang {
            Lang::En => format!("{}. {}", self.summary, self.action),
            Lang::ZhCn => format!("{}。{}", self.summary, self.action),
        }
    }
}

struct Rule {
    needles: &'static [&'static str],
    zh: (&'static str, &'static str),
    en: (&'static str, &'static str),
}

const RULES: &[Rule] = &[
    Rule {
        needles: &["moov atom not found"],
        zh: ("文件不完整或已损坏", "常见于拍摄中断或复制没有完成。用原设备重新导出，或用 untrunc 之类的工具修复"),
        en: (
            "The file is incomplete or damaged",
            "This usually comes from an interrupted recording or copy. Export it again or repair it with a tool like untrunc",
        ),
    },
    Rule {
        needles: &["no such file or directory", "the system cannot find the"],
        zh: ("找不到文件", "文件可能被移动、改名或删除了，重新导入"),
        en: ("File not found", "It may have been moved, renamed or deleted. Import it again"),
    },
    Rule {
        needles: &["permission denied", "access is denied", "operation not permitted"],
        zh: ("没有访问权限", "检查源文件与输出目录的权限，或在设置里换一个输出目录"),
        en: (
            "Permission denied",
            "Check the permissions of the source and output folders, or pick another output folder in Settings",
        ),
    },
    Rule {
        needles: &["no space left on device", "not enough space", "disk full"],
        zh: ("磁盘空间不足", "清理输出所在的磁盘，或在设置里换一个输出目录"),
        en: ("The disk is full", "Free up space on the output drive or pick another output folder in Settings"),
    },
    Rule {
        needles: &["out of memory", "cannot allocate memory"],
        zh: ("内存或显存不足", "关掉其他占用显存的程序，或把 GPU 并发数调到 1 后重试"),
        en: ("Out of memory", "Close other programs that use GPU memory, or set GPU concurrency to 1 and retry"),
    },
    Rule {
        needles: &["experimental codecs are not enabled"],
        zh: ("选中的编码器是实验性的", "换一种编码格式，或把编码器改回自动"),
        en: ("The selected encoder is experimental", "Switch to another format or set the encoder back to automatic"),
    },
    Rule {
        needles: &["unrecognized option", "option not found"],
        zh: ("附加参数里有 ffmpeg 不认识的选项", "检查“更多参数”里的附加 ffmpeg 参数"),
        en: (
            "The extra arguments contain an option ffmpeg does not know",
            "Check the extra ffmpeg arguments under More options",
        ),
    },
    Rule {
        needles: &["error setting option", "unable to parse", "invalid value"],
        zh: ("参数值不被接受", "检查附加参数与 x265-params 的写法，或把参数改回推荐值"),
        en: (
            "A parameter value was rejected",
            "Check the extra arguments and x265-params, or restore the recommended values",
        ),
    },
    Rule {
        needles: &["impossible to convert between the formats", "error reinitializing filters", "failed to configure"],
        zh: ("滤镜无法处理这段画面", "在“更多参数”里换一条色调映射管线，或关掉缩放后重试"),
        en: (
            "A filter could not process the video",
            "Pick another tone mapping pipeline under More options, or turn off scaling and retry",
        ),
    },
    Rule {
        needles: &["error while opening encoder", "could not open encoder", "error initializing output stream"],
        zh: ("编码器无法按这组参数启动", "把编码器改回自动，或降低位深、去掉附加参数后重试"),
        en: (
            "The encoder could not start with these settings",
            "Set the encoder back to automatic, or lower the bit depth and remove extra arguments",
        ),
    },
    Rule {
        needles: &["error while decoding", "invalid nal unit", "corrupt decoded frame", "error decoding"],
        zh: ("源文件里有损坏的片段", "用播放器检查源文件；能播放的话，把硬件解码关掉后重试"),
        en: (
            "The source contains damaged data",
            "Check the source in a player; if it plays, turn off hardware decoding and retry",
        ),
    },
    Rule {
        needles: &["invalid data found when processing input"],
        zh: ("不是可识别的媒体文件，或文件已损坏", "确认这是一个完整的视频文件"),
        en: ("Not a recognizable media file, or the file is damaged", "Make sure this is a complete video file"),
    },
];

/// 解释一段 stderr；没有命中任何已知情况时给通用说明，原文照样保留
pub fn explain(stderr: &str, lang: Lang) -> Explained {
    let lower = stderr.to_ascii_lowercase();
    let raw = key_line(stderr);
    let (summary, action) = RULES
        .iter()
        .find(|r| r.needles.iter().any(|n| lower.contains(n)))
        .map(|r| if lang == Lang::En { r.en } else { r.zh })
        .unwrap_or(match lang {
            Lang::En => {
                ("ffmpeg failed", "Expand the original message for details, or copy the command when asking for help")
            }
            Lang::ZhCn => ("ffmpeg 执行失败", "展开原文查看细节，求助时可以复制命令"),
        });
    Explained { summary: summary.into(), action: action.into(), raw }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_errors_get_a_cause_and_an_action_in_both_languages() {
        let e =
            explain("[mov,mp4 @ 0x1] moov atom not found\nx.mov: Invalid data found when processing input", Lang::ZhCn);
        assert_eq!(e.summary, "文件不完整或已损坏");
        assert!(e.action.contains("untrunc"));
        assert_eq!(e.raw, "[mov,mp4 @ 0x1] moov atom not found");
        let en = explain("av_interleaved_write_frame(): No space left on device", Lang::En);
        assert_eq!(en.summary, "The disk is full");
        assert!(en.sentence(Lang::En).starts_with("The disk is full. Free up"));
        assert!(
            explain("Error opening output D:\\x.mkv: Permission denied", Lang::ZhCn)
                .sentence(Lang::ZhCn)
                .starts_with("没有访问权限。")
        );
    }

    #[test]
    fn unknown_errors_keep_the_original() {
        let e = explain("something nobody has seen before", Lang::ZhCn);
        assert_eq!(e.summary, "ffmpeg 执行失败");
        assert_eq!(e.raw, "something nobody has seen before");
    }
}
