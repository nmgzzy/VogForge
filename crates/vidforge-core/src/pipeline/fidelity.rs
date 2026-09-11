//! 保真度约束求解（设计文档 4.4）：用户勾选的每一项能不能保留、为什么、怎么修。文字按界面语言生成。

use crate::i18n::{Lang, pick};
use crate::model::{
    AudioMode, Capabilities, Codec, Container, DoviAction, FidelityItem, FidelityKind, FidelityState, Fix, HdrAction,
    HdrKind, MediaInfo, Scenario, StreamAction, SubtitleMode, TrackRole, TranscodePlan,
};
use crate::tr;

use super::container::audio_fits;
use super::encoders::{supports_10bit, writes_hdr10};

/// 展示顺序
pub const FIDELITY_ORDER: [FidelityKind; 8] = [
    FidelityKind::DolbyVision,
    FidelityKind::Hdr10,
    FidelityKind::Hdr10plus,
    FidelityKind::Lossless,
    FidelityKind::AllAudio,
    FidelityKind::AllSubtitles,
    FidelityKind::Chapters,
    FidelityKind::TenBit,
];

/// 勾选项的名字
pub fn fidelity_label(kind: FidelityKind, lang: Lang) -> &'static str {
    match kind {
        FidelityKind::DolbyVision => pick(lang, "杜比视界", "Dolby Vision"),
        FidelityKind::Hdr10 => pick(lang, "HDR（HDR10 / HLG）", "HDR (HDR10 / HLG)"),
        FidelityKind::Hdr10plus => "HDR10+",
        FidelityKind::Lossless => pick(lang, "全景声与无损音轨", "Atmos & lossless audio"),
        FidelityKind::AllAudio => pick(lang, "全部音轨", "All audio tracks"),
        FidelityKind::AllSubtitles => pick(lang, "全部字幕", "All subtitles"),
        FidelityKind::Chapters => pick(lang, "章节", "Chapters"),
        FidelityKind::TenBit => pick(lang, "10bit 色深", "10-bit color"),
    }
}

struct Ctx {
    lang: Lang,
}

impl Ctx {
    fn l(&self, zh: &'static str, en: &'static str) -> &'static str {
        pick(self.lang, zh, en)
    }

    fn item(
        &self,
        kind: FidelityKind,
        state: FidelityState,
        detail: impl Into<String>,
        fixes: Vec<Fix>,
    ) -> FidelityItem {
        FidelityItem { kind, label: fidelity_label(kind, self.lang).to_string(), state, detail: detail.into(), fixes }
    }

    fn remux_fix(&self) -> Fix {
        fix("remux", self.l("改为原样封装", "Switch to Remux"))
    }

    /// 多个原因之间的分隔
    fn join(&self, parts: &[String]) -> String {
        parts.join(self.l("；", "; "))
    }

    /// 修正按钮上附带的改动清单
    fn with_changes(&self, base: &str, changes: &[&str]) -> String {
        if changes.is_empty() {
            base.to_string()
        } else {
            match self.lang {
                Lang::En => format!("{base} ({})", changes.join(" · ")),
                Lang::ZhCn => format!("{base}（{}）", changes.join(" · ")),
            }
        }
    }
}

fn fix(id: &str, label: impl Into<String>) -> Fix {
    Fix { id: id.to_string(), label: label.into() }
}

fn dv_label(profile: u8, bl_compat_id: u8) -> String {
    if profile == 8 { format!("8.{bl_compat_id}") } else { profile.to_string() }
}

fn upper(s: &str) -> String {
    s.to_uppercase()
}

pub fn resolve_fidelity(m: &MediaInfo, p: &TranscodePlan, caps: &Capabilities, lang: Lang) -> Vec<FidelityItem> {
    let c = Ctx { lang };
    FIDELITY_ORDER
        .iter()
        .map(|kind| match kind {
            FidelityKind::DolbyVision => dolby_vision(&c, m, p, caps),
            FidelityKind::Hdr10 => hdr10(&c, m, p),
            FidelityKind::Hdr10plus => hdr10plus(&c, m, p),
            FidelityKind::Lossless => lossless(&c, m, p),
            FidelityKind::AllAudio => all_audio(&c, m, p),
            FidelityKind::AllSubtitles => all_subtitles(&c, m, p),
            FidelityKind::Chapters => chapters(&c, m, p),
            FidelityKind::TenBit => ten_bit(&c, m, p, caps),
        })
        .collect()
}

fn dolby_vision(c: &Ctx, m: &MediaInfo, p: &TranscodePlan, caps: &Capabilities) -> FidelityItem {
    use FidelityState::*;
    let (k, lang) = (FidelityKind::DolbyVision, c.lang);
    let Some(dv) = m.video.first().and_then(|v| v.dolby_vision.as_ref()) else {
        return c.item(k, NotApplicable, c.l("源文件不含杜比视界", "The source has no Dolby Vision"), vec![]);
    };
    let name = format!("Profile {}", dv_label(dv.profile, dv.bl_compat_id));
    let vp = &p.video;
    if vp.action == StreamAction::Copy {
        let detail = tr!(
            lang,
            "原样封装完整保留 {} 的全部数据，包括增强层",
            "Remux keeps all {} data, including the enhancement layer",
            name
        );
        return c.item(k, Achievable, detail, vec![]);
    }
    if dv.has_enhancement_layer {
        let el = match dv.el_type {
            Some(crate::model::ElType::Mel) => "MEL",
            Some(crate::model::ElType::Fel) => "FEL",
            None => "EL",
        };
        let detail = tr!(
            lang,
            "源为 {} 双层（{}）。ffmpeg 无法编码增强层，重编码只能降级为 8.1 并丢失增强层的亮度与色度映射。要完整保留，请改为原样封装。",
            "The source is dual-layer {} ({}). ffmpeg cannot encode the enhancement layer, so re-encoding downgrades to 8.1 and loses its luma and chroma mapping. Use Remux to keep everything.",
            name,
            el
        );
        return c.item(k, Impossible, detail, vec![c.remux_fix()]);
    }
    if !caps.dolby_vision_encode {
        let detail = c
            .l("当前 ffmpeg 低于 7.1，无法写入杜比视界", "This ffmpeg is older than 7.1 and cannot write Dolby Vision");
        return c.item(k, Impossible, detail, vec![c.remux_fix()]);
    }
    let mut blockers: Vec<String> = Vec::new();
    let mut changes = Vec::new();
    if vp.dovi != DoviAction::Preserve {
        blockers.push(c.l("当前设置为不保留", "It is currently set not to keep it").into());
    }
    if vp.encoder.is_hardware() {
        blockers.push(c.l("硬件编码器无法输出杜比视界", "Hardware encoders cannot output Dolby Vision").into());
        changes.push(c.l("改用 CPU 编码", "encode on CPU"));
    }
    if vp.codec == Codec::H264 {
        blockers
            .push(c.l("H.264 的杜比视界几乎没有播放器支持", "Almost no player supports Dolby Vision in H.264").into());
        changes.push(c.l("改用 HEVC", "use HEVC"));
    }
    if vp.hdr_action == HdrAction::Tonemap {
        blockers.push(c.l("当前会色调映射为 SDR", "It is currently tone mapped to SDR").into());
        changes.push(c.l("保留 HDR", "keep HDR"));
    }
    if vp.bit_depth != 10 {
        blockers.push(c.l("杜比视界要求 10bit", "Dolby Vision requires 10-bit").into());
        changes.push("10bit");
    }
    if !blockers.is_empty() {
        let label = c.with_changes(c.l("开启保留", "Keep it"), &changes);
        return c.item(k, NeedsChange, c.join(&blockers), vec![fix("dovi_preserve", label)]);
    }
    let mut detail = tr!(
        lang,
        "保留 {}。ffmpeg 解析源的 RPU 后重新生成，逐帧动态元数据完整保留。",
        "Keeps {}. ffmpeg parses the source RPU and regenerates it, keeping all per-frame dynamic metadata.",
        name
    );
    if p.container != Container::Mkv {
        detail.push_str(c.l(
            " MP4/MOV 输出会自动添加 hvc1 标记与 -strict unofficial。",
            " MP4/MOV output automatically gets the hvc1 tag and -strict unofficial.",
        ));
    }
    if dv.profile == 5 {
        detail.push_str(c.l(
            " 注意：Profile 5 没有 HDR10 回退层，不支持杜比视界的设备会显示偏色。",
            " Note: Profile 5 has no HDR10 fallback layer, so devices without Dolby Vision show wrong colors.",
        ));
    }
    c.item(k, Achievable, detail, vec![])
}

fn hdr10(c: &Ctx, m: &MediaInfo, p: &TranscodePlan) -> FidelityItem {
    use FidelityState::*;
    let (k, lang) = (FidelityKind::Hdr10, c.lang);
    let Some(v) = m.video.first().filter(|v| v.color.hdr_kind != HdrKind::None) else {
        return c.item(k, NotApplicable, c.l("源为 SDR 视频", "The source is SDR"), vec![]);
    };
    let is_hlg = v.color.hdr_kind == HdrKind::Hlg;
    let vp = &p.video;
    if vp.action == StreamAction::Copy {
        return c.item(k, Achievable, c.l("原样封装完整保留 HDR 信息", "Remux keeps all HDR information"), vec![]);
    }
    let mut blockers: Vec<String> = Vec::new();
    let mut changes = Vec::new();
    if vp.hdr_action == HdrAction::Tonemap {
        blockers.push(c.l("当前会色调映射为 SDR", "It is currently tone mapped to SDR").into());
        changes.push(c.l("保留 HDR", "keep HDR"));
    }
    if vp.bit_depth != 10 {
        blockers.push(c.l("8bit 输出 HDR 会产生明显色带", "HDR in 8-bit shows visible banding").into());
        changes.push("10bit");
    }
    if !is_hlg && !writes_hdr10(vp.encoder) {
        blockers.push(tr!(
            lang,
            "{} 不会把 HDR10 元数据写入码流",
            "{} does not write HDR10 metadata into the stream",
            vp.encoder.name()
        ));
        changes.push(if vp.codec == Codec::H264 {
            c.l("改用 HEVC", "use HEVC")
        } else {
            c.l("改用 CPU 编码", "encode on CPU")
        });
    }
    if !blockers.is_empty() {
        let label = c.with_changes(c.l("保留 HDR", "Keep HDR"), &changes);
        return c.item(k, NeedsChange, c.join(&blockers), vec![fix("keep_hdr", label)]);
    }
    if is_hlg {
        let detail = c.l(
            "保留 HLG 色彩标记。HLG 不依赖额外元数据，电视与手机可直接识别。",
            "Keeps the HLG color tags. HLG needs no extra metadata; TVs and phones recognize it directly.",
        );
        return c.item(k, Achievable, detail, vec![]);
    }
    let detail = if vp.encoder.is_hardware() {
        tr!(
            lang,
            "保留 HDR10。{} 会把母版显示与 MaxCLL 写入码流（已在本机实测验证）。",
            "Keeps HDR10. {} writes the mastering display and MaxCLL into the stream (verified on real hardware).",
            vp.encoder.vendor().label()
        )
    } else {
        c.l(
            "保留 HDR10。软件编码器会自动透传母版显示与 MaxCLL 元数据。",
            "Keeps HDR10. Software encoders pass the mastering display and MaxCLL metadata through automatically.",
        )
        .to_string()
    };
    c.item(k, Achievable, detail, vec![])
}

fn hdr10plus(c: &Ctx, m: &MediaInfo, p: &TranscodePlan) -> FidelityItem {
    use FidelityState::*;
    let k = FidelityKind::Hdr10plus;
    if !m.video.first().is_some_and(|v| v.hdr10plus) {
        return c.item(k, NotApplicable, c.l("源文件不含 HDR10+", "The source has no HDR10+"), vec![]);
    }
    if p.video.action == StreamAction::Copy {
        return c.item(k, Achievable, c.l("原样封装完整保留", "Remux keeps it intact"), vec![]);
    }
    let detail = c.l(
        "ffmpeg 无法把 HDR10+ 动态元数据透传给编码器，需借助 x265 命令行或事后注入（v2 支持）。重编码后仍会保留 HDR10 基础层。",
        "ffmpeg cannot pass HDR10+ dynamic metadata to the encoder; that needs the x265 CLI or injecting it afterwards (planned for v2). Re-encoding still keeps the HDR10 base layer.",
    );
    c.item(k, Impossible, detail, vec![c.remux_fix()])
}

fn audio_name(a: &crate::model::AudioStream) -> String {
    a.title.clone().unwrap_or_else(|| upper(&a.codec))
}

fn lossless(c: &Ctx, m: &MediaInfo, p: &TranscodePlan) -> FidelityItem {
    use FidelityState::*;
    let (k, lang) = (FidelityKind::Lossless, c.lang);
    let hq: Vec<_> = m.audio.iter().filter(|a| a.lossless || a.atmos).collect();
    if hq.is_empty() {
        return c.item(
            k,
            NotApplicable,
            c.l("源文件没有无损或全景声音轨", "The source has no lossless or Atmos track"),
            vec![],
        );
    }
    let mut blockers = Vec::new();
    let mut need_mkv = false;
    for a in &hq {
        let name = audio_name(a);
        let copied = p.audio.iter().any(|t| t.source_index == a.index && t.action == StreamAction::Copy);
        if !copied {
            blockers.push(if a.atmos {
                tr!(
                    lang,
                    "「{}」会被重新编码，全景声元数据将丢失（Atmos 无法重新编码）",
                    "\"{}\" would be re-encoded and lose its Atmos metadata (Atmos cannot be re-encoded)",
                    name
                )
            } else {
                tr!(lang, "「{}」会被有损压缩", "\"{}\" would be lossy compressed", name)
            });
        } else if !audio_fits(&a.codec, p.container) {
            blockers.push(tr!(
                lang,
                "{} 不支持 {}",
                "{} does not support {}",
                upper(p.container.ext()),
                upper(&a.codec)
            ));
            need_mkv = true;
        }
    }
    if !blockers.is_empty() {
        let label = if need_mkv || p.audio_mode == AudioMode::CompatOnly {
            c.l("原样保留（切换到 MKV · 追加兼容轨）", "Keep as-is (switch to MKV · add a compatible track)")
        } else {
            c.l("原样保留", "Keep as-is")
        };
        return c.item(k, NeedsChange, c.join(&blockers), vec![fix("keep_lossless", label)]);
    }
    let compat = p.audio.iter().any(|t| t.role == TrackRole::Compat);
    let names = hq.iter().map(|a| audio_name(a)).collect::<Vec<_>>().join(c.l("、", ", "));
    let detail = if compat {
        tr!(
            lang,
            "原样复制 {}，并额外生成兼容轨，手机与耳机也能播放",
            "Copies {} as-is and adds a compatible track for phones and headphones",
            names
        )
    } else {
        tr!(lang, "原样复制 {}", "Copies {} as-is", names)
    };
    c.item(k, Achievable, detail, vec![])
}

fn all_audio(c: &Ctx, m: &MediaInfo, p: &TranscodePlan) -> FidelityItem {
    use FidelityState::*;
    let (k, lang) = (FidelityKind::AllAudio, c.lang);
    if m.audio.len() <= 1 {
        let why = if m.audio.is_empty() {
            c.l("源文件没有音轨", "The source has no audio")
        } else {
            c.l("源文件只有一条音轨", "The source has only one audio track")
        };
        return c.item(k, NotApplicable, why, vec![]);
    }
    let missing: Vec<_> = m.audio.iter().filter(|a| !p.audio.iter().any(|t| t.source_index == a.index)).collect();
    let unfit = p
        .audio
        .iter()
        .filter(|t| {
            t.action == StreamAction::Copy
                && m.audio
                    .iter()
                    .find(|a| a.index == t.source_index)
                    .is_some_and(|a| !audio_fits(&a.codec, p.container))
        })
        .count();
    if missing.is_empty() && unfit == 0 {
        let detail = tr!(lang, "保留全部 {} 条音轨", "Keeps all {} audio tracks", m.audio.len());
        return c.item(k, Achievable, detail, vec![]);
    }
    let mut parts = Vec::new();
    if !missing.is_empty() {
        let names = missing.iter().map(|a| a.title.clone().unwrap_or_else(|| a.codec.clone())).collect::<Vec<_>>();
        let list = names.join(c.l("、", ", "));
        parts.push(tr!(lang, "将丢弃 {} 条：{}", "{} track(s) would be dropped: {}", missing.len(), list));
    }
    if unfit > 0 {
        parts.push(tr!(
            lang,
            "{} 条音轨的编码不被 {} 支持",
            "{} track(s) use a codec {} does not support",
            unfit,
            upper(p.container.ext())
        ));
    }
    let label = if unfit > 0 || p.container == Container::Mp4 {
        c.l("复制全部（切换到 MKV）", "Copy all (switch to MKV)")
    } else {
        c.l("复制全部音轨", "Copy all audio tracks")
    };
    c.item(k, NeedsChange, c.join(&parts), vec![fix("audio_copy_all", label)])
}

fn all_subtitles(c: &Ctx, m: &MediaInfo, p: &TranscodePlan) -> FidelityItem {
    use FidelityState::*;
    let (k, lang) = (FidelityKind::AllSubtitles, c.lang);
    if m.subtitle.is_empty() {
        return c.item(k, NotApplicable, c.l("源文件没有字幕", "The source has no subtitles"), vec![]);
    }
    let image = m.subtitle.iter().filter(|s| s.image_based).count();
    let blocker = match p.subtitles {
        SubtitleMode::None => Some(c.l("当前设置不保留字幕", "Subtitles are currently not kept").to_string()),
        SubtitleMode::TextOnly if image > 0 => {
            Some(tr!(lang, "将丢弃 {} 条图形字幕（PGS）", "{} image subtitle track(s) (PGS) would be dropped", image))
        }
        _ if p.container != Container::Mkv && image > 0 => Some(tr!(
            lang,
            "{} 不支持 PGS 图形字幕",
            "{} does not support PGS image subtitles",
            upper(p.container.ext())
        )),
        _ => None,
    };
    if let Some(b) = blocker {
        let label = if image > 0 {
            c.l("保留全部（切换到 MKV）", "Keep all (switch to MKV)")
        } else {
            c.l("保留全部字幕", "Keep all subtitles")
        };
        return c.item(k, NeedsChange, b, vec![fix("subs_all", label)]);
    }
    let detail = tr!(lang, "保留全部 {} 条字幕", "Keeps all {} subtitle tracks", m.subtitle.len());
    c.item(k, Achievable, detail, vec![])
}

fn chapters(c: &Ctx, m: &MediaInfo, p: &TranscodePlan) -> FidelityItem {
    let (k, lang) = (FidelityKind::Chapters, c.lang);
    if m.chapters == 0 {
        return c.item(k, FidelityState::NotApplicable, c.l("源文件没有章节", "The source has no chapters"), vec![]);
    }
    let detail = if p.container == Container::Mkv {
        tr!(lang, "保留 {} 个章节", "Keeps {} chapters", m.chapters)
    } else {
        tr!(
            lang,
            "保留 {} 个章节（MP4/MOV 的章节在部分播放器中不显示）",
            "Keeps {} chapters (some players do not show MP4/MOV chapters)",
            m.chapters
        )
    };
    c.item(k, FidelityState::Achievable, detail, vec![])
}

fn ten_bit(c: &Ctx, m: &MediaInfo, p: &TranscodePlan, caps: &Capabilities) -> FidelityItem {
    use FidelityState::*;
    let (k, lang) = (FidelityKind::TenBit, c.lang);
    if m.video.first().is_none_or(|v| v.bit_depth < 10) {
        return c.item(k, NotApplicable, c.l("源为 8bit 视频", "The source is 8-bit"), vec![]);
    }
    let vp = &p.video;
    if vp.action == StreamAction::Copy {
        return c.item(k, Achievable, c.l("原样封装保持原始位深", "Remux keeps the original bit depth"), vec![]);
    }
    let supported = supports_10bit(vp.encoder, caps);
    if vp.bit_depth == 10 && supported {
        return c.item(k, Achievable, c.l("输出 10bit", "Outputs 10-bit"), vec![]);
    }
    let why = if supported {
        c.l("当前输出 8bit", "It currently outputs 8-bit").to_string()
    } else {
        tr!(lang, "{} 不支持 10bit", "{} does not support 10-bit", vp.encoder.name())
    };
    c.item(k, NeedsChange, why, vec![fix("ten_bit", c.l("改为 10bit", "Switch to 10-bit"))])
}

/// 把一键修正应用到计划上。调用方随后应再做一次 normalize
pub fn apply_fix(mut plan: TranscodePlan, fix_id: &str, media: &MediaInfo) -> TranscodePlan {
    let has_image_subs = media.subtitle.iter().any(|s| s.image_based);
    let vp = &mut plan.video;
    match fix_id {
        "remux" => {
            plan.scenario = Scenario::Remux;
            vp.action = StreamAction::Copy;
            vp.dovi = DoviAction::Remux;
            vp.hdr_action = HdrAction::Keep;
            plan.audio_mode = AudioMode::CopyAll;
            plan.subtitles = SubtitleMode::All;
            plan.container = Container::Mkv;
        }
        "dovi_preserve" => {
            vp.dovi = DoviAction::Preserve;
            vp.encoder_auto = true;
            vp.hdr_action = HdrAction::Keep;
            vp.bit_depth = 10;
            if vp.codec == Codec::H264 {
                vp.codec = Codec::Hevc;
            }
            plan.fidelity.dolby_vision = true;
        }
        "keep_hdr" => {
            vp.hdr_action = HdrAction::Keep;
            vp.bit_depth = 10;
            if vp.codec == Codec::H264 {
                vp.codec = Codec::Hevc;
            }
            if !writes_hdr10(vp.encoder) {
                vp.encoder_auto = true;
            }
            plan.fidelity.hdr10 = true;
        }
        "keep_lossless" => {
            if plan.audio_mode == AudioMode::CompatOnly {
                plan.audio_mode = AudioMode::OriginalPlusCompat;
            }
            plan.container = Container::Mkv;
            plan.fidelity.lossless = true;
        }
        "audio_copy_all" => {
            plan.audio_mode = AudioMode::CopyAll;
            plan.container = Container::Mkv;
            plan.fidelity.all_audio = true;
        }
        "subs_all" => {
            plan.subtitles = SubtitleMode::All;
            if has_image_subs {
                plan.container = Container::Mkv;
            }
            plan.fidelity.all_subtitles = true;
        }
        "ten_bit" => {
            vp.bit_depth = 10;
            vp.encoder_auto = true;
            plan.fidelity.ten_bit = true;
        }
        _ => {}
    }
    plan
}
