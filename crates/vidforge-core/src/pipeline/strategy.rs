//! 场景策略引擎（设计文档 4.5）：素材 + 场景 → 自洽的转码计划。
//!
//! 表驱动：每个场景一份档案（编码格式、画质、容器、音频策略……），再按素材特征与环境能力调整。
//! [`normalize_plan`] 在任何字段被修改后把计划修回自洽状态。

use crate::model::{
    AudioCodec, AudioMode, AudioStream, AudioTrackPlan, Capabilities, Codec, Container, DoviAction, EncoderId,
    FidelityRequest, FpsPolicy, HdrAction, HdrKind, MediaInfo, QualityTier, RateControl, ResolutionPreset, Scenario,
    SourceHint, StreamAction, SubtitleMode, TrackRole, TranscodePlan, VideoPlan,
};

use super::container::audio_fits;
use super::encoders::{
    EncoderNeeds, codec_available, default_preset, pick_encoder, preset_options, quality_value, supports_10bit,
    supports_rate_control,
};
use super::estimate::source_video_bps;
use super::fps::{recommend_cfr_target, source_rate};

/// 场景默认格式在当前 ffmpeg 里编不了时，按这个顺序换一种
const CODEC_FALLBACK: [Codec; 3] = [Codec::Hevc, Codec::H264, Codec::Av1];

/// 码率输入的合理范围（kbps）：再低画面不可看，再高超过任何编码级别
pub const MIN_KBPS: u32 = 100;
pub const MAX_KBPS: u32 = 400_000;

/// 目标码率的上限：不高于源视频码率（不升档，设计文档 4.5），并落在输入范围内。源码率未知时只受输入范围约束
pub fn max_target_kbps(media: &MediaInfo) -> u32 {
    let source = (source_video_bps(media) / 1000.0).floor() as u32;
    if source == 0 { MAX_KBPS } else { source.clamp(MIN_KBPS, MAX_KBPS) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CfrPolicy {
    Never,
    IfVfr,
    Always,
}

struct Profile {
    codec: Codec,
    prefer_hw: bool,
    quality: QualityTier,
    resolution: ResolutionPreset,
    tonemap: bool,
    container: Container,
    audio_mode: AudioMode,
    subtitles: SubtitleMode,
    cfr: CfrPolicy,
    short_gop: bool,
    keep_dv: bool,
}

fn is_hdr(media: &MediaInfo) -> bool {
    media.video.first().is_some_and(|v| v.color.hdr_kind != HdrKind::None)
}

fn profile_for(scenario: Scenario, media: &MediaInfo) -> Profile {
    let hdr = is_hdr(media);
    let has_lossless = media.audio.iter().any(|a| a.lossless || a.atmos);
    let base = Profile {
        codec: Codec::Hevc,
        prefer_hw: false,
        quality: QualityTier::High,
        resolution: ResolutionPreset::Source,
        tonemap: false,
        container: Container::Mkv,
        audio_mode: AudioMode::CopyAll,
        subtitles: SubtitleMode::All,
        cfr: CfrPolicy::Never,
        short_gop: false,
        keep_dv: false,
    };
    match scenario {
        Scenario::Archive => Profile {
            audio_mode: if has_lossless { AudioMode::OriginalPlusCompat } else { AudioMode::CopyAll },
            keep_dv: true,
            ..base
        },
        Scenario::Collection => Profile { quality: QualityTier::Lossless, keep_dv: true, ..base },
        Scenario::Streaming => Profile {
            prefer_hw: true,
            quality: QualityTier::Standard,
            container: Container::Mp4,
            audio_mode: AudioMode::CompatOnly,
            subtitles: SubtitleMode::TextOnly,
            cfr: CfrPolicy::IfVfr,
            ..base
        },
        Scenario::Mobile => Profile {
            codec: Codec::H264,
            prefer_hw: true,
            quality: QualityTier::Standard,
            resolution: ResolutionPreset::P1080,
            tonemap: hdr,
            container: Container::Mp4,
            audio_mode: AudioMode::CompatOnly,
            subtitles: SubtitleMode::TextOnly,
            ..base
        },
        Scenario::Social => Profile {
            codec: Codec::H264,
            prefer_hw: true,
            resolution: ResolutionPreset::P1080,
            tonemap: hdr,
            container: Container::Mp4,
            audio_mode: AudioMode::CompatOnly,
            subtitles: SubtitleMode::None,
            cfr: CfrPolicy::Always,
            ..base
        },
        Scenario::Editing => Profile {
            codec: if hdr { Codec::Hevc } else { Codec::H264 },
            quality: QualityTier::Lossless,
            container: Container::Mov,
            subtitles: SubtitleMode::None,
            cfr: CfrPolicy::Always,
            short_gop: true,
            ..base
        },
        Scenario::Smallest => Profile {
            codec: Codec::Av1,
            quality: QualityTier::Small,
            resolution: ResolutionPreset::P1080,
            audio_mode: AudioMode::CompatOnly,
            subtitles: SubtitleMode::TextOnly,
            ..base
        },
        Scenario::Remux => Profile { keep_dv: true, ..base },
    }
}

/// 场景对这个素材默认采用的编码格式
pub fn scenario_codec(scenario: Scenario, media: &MediaInfo) -> Codec {
    profile_for(scenario, media).codec
}

/// 按源文件特征推荐的起始场景。已高度压缩的片源推荐原样封装，避免二次有损
pub fn suggest_scenario(media: &MediaInfo) -> Scenario {
    match media.source_hint {
        SourceHint::Bluray => Scenario::Collection,
        SourceHint::Streaming => Scenario::Remux,
        SourceHint::Screen => Scenario::Editing,
        _ => Scenario::Archive,
    }
}

pub fn prefer_hw_for(scenario: Scenario) -> bool {
    matches!(scenario, Scenario::Streaming | Scenario::Mobile | Scenario::Social)
}

pub fn recommend(media: &MediaInfo, scenario: Scenario, caps: &Capabilities) -> TranscodePlan {
    let v = media.video.first();
    let p = profile_for(scenario, media);
    let hdr = is_hdr(media);
    // 单层杜比视界才能在重编码时保留；P7 双层只能原样封装
    let dv_preservable = v.and_then(|v| v.dolby_vision.as_ref()).is_some_and(|dv| !dv.has_enhancement_layer);
    let want_dv = p.keep_dv && dv_preservable;
    // 想转 SDR 却没有任何可用的色调映射管线时，只能保留 HDR（explain 会给出警告）
    let tonemap = if hdr && p.tonemap { caps.pick_tonemap() } else { None };
    let keep_hdr = hdr && tonemap.is_none();
    let bit_depth: u8 = if keep_hdr || matches!(scenario, Scenario::Archive | Scenario::Collection) { 10 } else { 8 };
    let need_hdr10 = keep_hdr && v.is_some_and(|v| v.color.hdr_kind == HdrKind::Hdr10);
    let pick = pick_encoder(
        p.codec,
        &EncoderNeeds {
            prefer_hw: p.prefer_hw,
            need_10bit: bit_depth == 10,
            need_dv: want_dv,
            need_hdr10,
            need_two_pass: false,
        },
        caps,
    );
    let fps_target = v.map_or(30.0, recommend_cfr_target);
    let use_cfr = v.is_some_and(|v| p.cfr == CfrPolicy::Always || (p.cfr == CfrPolicy::IfVfr && v.is_vfr));

    let plan = TranscodePlan {
        scenario,
        video: VideoPlan {
            action: if scenario == Scenario::Remux { StreamAction::Copy } else { StreamAction::Encode },
            codec: p.codec,
            encoder: pick.encoder,
            encoder_auto: true,
            quality: p.quality,
            quality_value: quality_value(pick.encoder, p.quality),
            rate_control: RateControl::Quality,
            preset: default_preset(pick.encoder, scenario).to_string(),
            bit_depth,
            resolution: p.resolution,
            fps: if use_cfr { FpsPolicy::Cfr { fps: fps_target } } else { FpsPolicy::Keep },
            hdr_action: if tonemap.is_some() { HdrAction::Tonemap } else { HdrAction::Keep },
            tonemap,
            dovi: if scenario == Scenario::Remux {
                DoviAction::Remux
            } else if want_dv {
                DoviAction::Preserve
            } else {
                DoviAction::Disable
            },
            gop: (p.short_gop && v.is_some()).then(|| ((fps_target / 2.0).round() as u32).max(1)),
            extra_params: None,
            extra_args: None,
        },
        audio: Vec::new(),
        audio_mode: p.audio_mode,
        loudnorm: false,
        subtitles: p.subtitles,
        container: p.container,
        fidelity: default_fidelity(media, scenario),
    };
    normalize_plan(plan, media, caps)
}

/// 按场景给出默认的保真度勾选：源里有什么、场景在意什么，就勾什么
pub fn default_fidelity(media: &MediaInfo, scenario: Scenario) -> FidelityRequest {
    let v = media.video.first();
    let keepy = matches!(scenario, Scenario::Archive | Scenario::Collection | Scenario::Remux);
    let hdr = is_hdr(media);
    FidelityRequest {
        dolby_vision: keepy && v.is_some_and(|v| v.dolby_vision.is_some()),
        hdr10: (keepy || matches!(scenario, Scenario::Streaming | Scenario::Editing)) && hdr,
        hdr10plus: keepy && v.is_some_and(|v| v.hdr10plus),
        lossless: keepy && media.audio.iter().any(|a| a.lossless || a.atmos),
        all_audio: matches!(scenario, Scenario::Collection | Scenario::Remux) && media.audio.len() > 1,
        all_subtitles: matches!(scenario, Scenario::Collection | Scenario::Remux) && !media.subtitle.is_empty(),
        chapters: keepy && media.chapters > 0,
        ten_bit: keepy && v.map_or(8, |v| v.bit_depth) >= 10,
    }
}

fn primary_audio(media: &MediaInfo) -> Option<&AudioStream> {
    media.audio.iter().find(|a| a.is_default).or(media.audio.first())
}

/// 重新编码的音轨不升档：码率不高于源（有损源且码率已知时；无损源的码率远高于任何有损码率）。
/// 源码率异常小时仍留 32k 的底，免得被错误的元数据压成不可听
fn audio_kbps(src: &AudioStream, kbps: u32) -> u32 {
    match src.bitrate {
        Some(b) if !src.lossless && b > 0 => (b / 1000).max(32).min(u64::from(kbps)) as u32,
        _ => kbps,
    }
}

/// 按音频策略生成输出音轨
pub fn build_audio_tracks(media: &MediaInfo, plan: &TranscodePlan, caps: &Capabilities) -> Vec<AudioTrackPlan> {
    let Some(primary) = primary_audio(media) else { return Vec::new() };
    let mut tracks = Vec::new();
    // 最小体积用 Opus；构建里没有 libopus 时退回 AAC（探测完成前按有处理，免得界面来回跳）
    let opus = caps.status != crate::model::EnvStatus::Ready
        || caps.build_flags.iter().any(|f| f.name == "libopus" && f.present);
    let stereo_codec = if plan.scenario == Scenario::Smallest && opus { AudioCodec::Opus } else { AudioCodec::Aac };
    let stereo_rate = match plan.scenario {
        Scenario::Smallest => 96,
        Scenario::Mobile => 160,
        _ => 256,
    };

    let stereo_compat = |src: &AudioStream| AudioTrackPlan {
        source_index: src.index,
        action: StreamAction::Encode,
        codec: Some(stereo_codec),
        bitrate_kbps: Some(audio_kbps(src, stereo_rate)),
        // 单声道源保持单声道，不升成立体声
        channels: Some(if src.channels == 1 { 1 } else { 2 }),
        title: if src.channels > 2 {
            // 生成轨的标题写进文件，用英文：任何语言的播放器都能读，计划也不随界面语言变化
            Some(format!("{} Stereo (downmix)", stereo_codec.name().to_uppercase()))
        } else {
            src.title.clone()
        },
        role: TrackRole::Compat,
    };

    // 容器装不下的音轨不能原样复制（例如 TrueHD 进 MOV / MP4），否则命令必然失败。改为重编码：
    // MOV 多用于剪辑，转 24bit PCM 保住音质；MP4 多声道转 E-AC-3、立体声转 AAC。保真度面板照常提示无损未保留
    let copy_of = |a: &AudioStream| -> AudioTrackPlan {
        if audio_fits(&a.codec, plan.container) {
            return AudioTrackPlan {
                source_index: a.index,
                action: StreamAction::Copy,
                codec: None,
                bitrate_kbps: None,
                channels: None,
                title: a.title.clone(),
                role: TrackRole::Original,
            };
        }
        if plan.container == Container::Mov {
            return AudioTrackPlan {
                source_index: a.index,
                action: StreamAction::Encode,
                codec: Some(AudioCodec::PcmS24le),
                bitrate_kbps: None,
                channels: None,
                title: a.title.clone(),
                role: TrackRole::Original,
            };
        }
        let multi = a.channels > 2;
        AudioTrackPlan {
            source_index: a.index,
            action: StreamAction::Encode,
            codec: Some(if multi { AudioCodec::Eac3 } else { AudioCodec::Aac }),
            bitrate_kbps: Some(audio_kbps(a, if multi { 640 } else { 256 })),
            channels: Some(if multi { a.channels.min(6) } else { a.channels }),
            title: a.title.clone(),
            role: TrackRole::Original,
        }
    };

    match plan.audio_mode {
        AudioMode::CopyAll => tracks.extend(media.audio.iter().map(copy_of)),
        AudioMode::OriginalPlusCompat => {
            tracks.extend(media.audio.iter().map(copy_of));
            if primary.lossless || primary.atmos || primary.channels > 2 {
                tracks.push(stereo_compat(primary));
            }
        }
        AudioMode::CompatOnly => {
            let single = matches!(plan.scenario, Scenario::Mobile | Scenario::Social | Scenario::Smallest);
            let sources: Vec<&AudioStream> = if single { vec![primary] } else { media.audio.iter().collect() };
            for a in sources {
                let lossy = !a.lossless && !a.atmos;
                let fits = audio_fits(&a.codec, plan.container);
                if lossy && fits && (!single || a.channels <= 2) && !(single && a.codec != stereo_codec.name()) {
                    tracks.push(copy_of(a));
                } else if !single && a.channels > 2 {
                    // 3–5 声道的源保持原声道数，不升成 5.1
                    let ch = a.channels.min(6);
                    let layout = if ch == 6 { "5.1".to_string() } else { format!("{ch}ch") };
                    tracks.push(AudioTrackPlan {
                        source_index: a.index,
                        action: StreamAction::Encode,
                        codec: Some(AudioCodec::Eac3),
                        bitrate_kbps: Some(audio_kbps(a, 640)),
                        channels: Some(ch),
                        title: Some(if a.atmos {
                            format!("DD+ {layout} (from Atmos, without Atmos metadata)")
                        } else {
                            format!("DD+ {layout}")
                        }),
                        role: TrackRole::Compat,
                    });
                } else {
                    tracks.push(stereo_compat(a));
                }
            }
            // 多声道主轨再追加一条立体声，保证耳机与手机可用
            if !single && primary.channels > 2 {
                tracks.push(stereo_compat(primary));
            }
        }
    }
    tracks
}

/// 换编码器（运行时回退用）：改为手选，质量数值与 preset 换成新编码器的对应值，再整理计划
pub fn switch_encoder(
    mut plan: TranscodePlan,
    encoder: EncoderId,
    media: &MediaInfo,
    caps: &Capabilities,
) -> TranscodePlan {
    let vp = &mut plan.video;
    vp.codec = encoder.codec();
    vp.encoder = encoder;
    vp.encoder_auto = false;
    vp.quality_value = quality_value(encoder, vp.quality);
    vp.preset = default_preset(encoder, plan.scenario).to_string();
    normalize_plan(plan, media, caps)
}

/// 让计划保持自洽。任何字段被修改后都应调用一次：编码格式变了要重选编码器、
/// 编码器变了要换质量数值与 preset、容器变了要重建音轨……
pub fn normalize_plan(mut plan: TranscodePlan, media: &MediaInfo, caps: &Capabilities) -> TranscodePlan {
    let scenario = plan.scenario;
    let vp = &mut plan.video;
    if vp.action == StreamAction::Encode {
        // 当前 ffmpeg 根本编不了这种格式时换一种能编的，否则会生成一条跑不起来的命令。环境完全不可用时不动
        if !codec_available(vp.codec, caps) {
            if let Some(alt) = CODEC_FALLBACK.into_iter().find(|&c| c != vp.codec && codec_available(c, caps)) {
                vp.codec = alt;
                vp.encoder_auto = true;
            }
        }
        // 手选的编码器在当前环境不可用（例如换了 ffmpeg），或与新编码格式不匹配，回到自动选择
        if !vp.encoder_auto && !caps.encoder_usable(vp.encoder) {
            vp.encoder_auto = true;
        }
        if !vp.encoder_auto && vp.encoder.codec() != vp.codec {
            vp.encoder_auto = true;
        }
        if vp.encoder_auto {
            let before = vp.encoder;
            let needs = EncoderNeeds {
                prefer_hw: prefer_hw_for(scenario),
                need_10bit: vp.bit_depth == 10,
                need_dv: vp.dovi == DoviAction::Preserve,
                need_hdr10: vp.hdr_action == HdrAction::Keep
                    && media.video.first().is_some_and(|v| v.color.hdr_kind == HdrKind::Hdr10),
                need_two_pass: matches!(vp.rate_control, RateControl::TwoPass { .. }),
            };
            vp.encoder = pick_encoder(vp.codec, &needs, caps).encoder;
            if before != vp.encoder {
                vp.quality_value = quality_value(vp.encoder, vp.quality);
                vp.preset = default_preset(vp.encoder, scenario).to_string();
            }
        }
        // 手选编码器后旧的 preset 可能不属于新编码器（x265 的 medium 对 NVENC 无效），换成该编码器的默认值
        if !preset_options(vp.encoder).contains(&vp.preset.as_str()) {
            vp.preset = default_preset(vp.encoder, scenario).to_string();
        }
        // 码率数值拉回合理范围，且不升档：平均码率不高于源视频码率，峰值不高于它的 1.5 倍（与"目标码率"
        // 模式的峰值 1.5 倍对应；QSV 的 QVBR 取峰值的 2/3 作平均码率）。
        // 编码器做不到的码率控制换成最接近的模式（手选了硬件编码器又要两遍时）
        let top = max_target_kbps(media);
        let peak = (top * 3 / 2).min(MAX_KBPS);
        vp.rate_control = match vp.rate_control {
            RateControl::Quality => RateControl::Quality,
            RateControl::Bitrate { kbps } => RateControl::Bitrate { kbps: kbps.clamp(MIN_KBPS, top) },
            RateControl::Capped { kbps } => RateControl::Capped { kbps: kbps.clamp(MIN_KBPS, peak) },
            RateControl::TwoPass { kbps } => RateControl::TwoPass { kbps: kbps.clamp(MIN_KBPS, top) },
        };
        if !supports_rate_control(vp.encoder, vp.rate_control) {
            vp.rate_control = match vp.rate_control {
                // 峰值换算成平均码率：与"目标码率"模式的峰值 1.5 倍对应
                RateControl::Capped { kbps } => RateControl::Bitrate { kbps: (kbps * 2 / 3).max(MIN_KBPS) },
                RateControl::TwoPass { kbps } => RateControl::Bitrate { kbps },
                other => other,
            };
        }
        // 不提帧率：固定帧率的目标不高于源（可变帧率源以名义帧率为准）。更高的帧率只是复制帧
        // 源帧率读不出时不封顶（否则会拉成 0 fps）；不合法的目标（0、负数）换成推荐值，绝不生成 -r 0
        if let FpsPolicy::Cfr { fps } = vp.fps {
            if !(fps.is_finite() && fps > 0.0) {
                vp.fps = FpsPolicy::Cfr { fps: media.video.first().map_or(30.0, recommend_cfr_target) };
            }
        }
        if let (FpsPolicy::Cfr { fps }, Some(source)) = (vp.fps, media.video.first().and_then(source_rate)) {
            if fps > source * (1.0 + 1e-9) {
                vp.fps = FpsPolicy::Cfr { fps: source };
            }
        }
        // 硬件编码器不支持 10bit 时降为 8bit，保真度面板会给出提示
        if vp.bit_depth == 10 && !supports_10bit(vp.encoder, caps) {
            vp.bit_depth = 8;
        }
        if vp.hdr_action == HdrAction::Tonemap && vp.tonemap.is_none_or(|t| !caps.tonemap_available(t)) {
            vp.tonemap = caps.pick_tonemap();
            if vp.tonemap.is_none() {
                vp.hdr_action = HdrAction::Keep;
            }
        }
        if vp.hdr_action != HdrAction::Tonemap {
            vp.tonemap = None;
        }
    }
    plan.audio = build_audio_tracks(media, &plan, caps);
    plan
}
