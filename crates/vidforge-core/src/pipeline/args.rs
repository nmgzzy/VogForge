//! 命令构建（设计文档 4.3）。每条规则都对应技术事实文档里的一条已核实事实，修改时同步文档与测试。
//!
//! 输出分段结构，界面按段换行展示，执行与复制时再拍平。全部是纯函数，不读环境。

use std::path::Path;

use crate::model::{
    ArgSegment, AudioStream, Capabilities, Container, DoviAction, EncoderId, FpsPolicy, HdrAction, MediaInfo,
    ResolutionPreset, StreamAction, SubtitleMode, ToneMapPipeline, TranscodePlan, VideoStream,
};

use super::fps::fps_arg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dimensions {
    pub w: u32,
    pub h: u32,
    pub portrait: bool,
}

/// 显示尺寸：带 90° / 270° 旋转（Display Matrix）的手机竖拍素材，编码尺寸是横的、显示是竖的
pub fn display_size(v: &VideoStream) -> (u32, u32) {
    if v.rotation.rem_euclid(180) == 90 { (v.height, v.width) } else { (v.width, v.height) }
}

/// 目标尺寸（显示方向）：按短边缩放、保持宽高比、边长取偶数；目标不小于源时返回 None（绝不放大）。
/// 重编码时 ffmpeg 先按 Display Matrix 自动旋转再进滤镜，所以缩放要按显示方向算
pub fn target_dimensions(v: &VideoStream, preset: ResolutionPreset) -> Option<Dimensions> {
    let target = preset.short_side()?;
    let (w, h) = display_size(v);
    let short = w.min(h);
    if short == 0 || target >= short {
        return None;
    }
    let portrait = h > w;
    let scale = f64::from(target) / f64::from(short);
    let even = |n: f64| ((n / 2.0).round() * 2.0) as u32;
    Some(if portrait {
        Dimensions { w: target, h: even(f64::from(h) * scale), portrait }
    } else {
        Dimensions { w: even(f64::from(w) * scale), h: target, portrait }
    })
}

/// 多声道降混立体声。5.1 与 7.1 的声道名不同，`pan` 引用不存在的声道会直接报错，所以按布局分别生成。
/// 中置提升约 3dB 让对白更清楚，alimiter 防止叠加后削波；LFE 不混入立体声
pub fn downmix_filter(src: &AudioStream) -> String {
    let layout = &src.channel_layout;
    let expr = if src.channels >= 8 || layout.contains("7.1") {
        "FL=0.707*FC+1.0*FL+0.6*BL+0.6*SL|FR=0.707*FC+1.0*FR+0.6*BR+0.6*SR"
    } else if layout.contains("side") {
        "FL=0.707*FC+1.0*FL+0.707*SL|FR=0.707*FC+1.0*FR+0.707*SR"
    } else {
        "FL=0.707*FC+1.0*FL+0.707*BL|FR=0.707*FC+1.0*FR+0.707*BR"
    };
    format!("pan=stereo|{expr},alimiter=limit=0.97:level=false")
}

/// 硬件解码参数。一律用 `-hwaccel auto`：解码后的帧自动下载到内存，软件滤镜与各家编码器都能接。
/// 不能按编码器写 `-hwaccel qsv`：9.0 起它默认把帧留在 GPU 上，后面再要求 `-pix_fmt p010le` 会转换失败
/// （技术事实文档 7.5 节，阶段 4 实测）
fn hwaccel_for(plan: &TranscodePlan, media: &MediaInfo) -> Vec<String> {
    let vp = &plan.video;
    if vp.action == StreamAction::Copy {
        return Vec::new();
    }
    // 硬解后的 hwdownload 可能丢失 DV RPU 与 HDR10+ 等 side data，保留这些时走全软件路径
    if vp.dovi == DoviAction::Preserve || media.video.first().is_some_and(|v| v.hdr10plus) {
        return Vec::new();
    }
    strings(&["-hwaccel", "auto"])
}

/// 转固定帧率时补齐视频尾部：源里视频比音频短半帧以上时，把最后一帧延长到音频结束。
/// CFR 只能填满到最后一帧结束，源本身的长度差会原样带进输出（阶段 4 实测差 2 帧），剪辑时就是音画不齐
pub fn tail_pad(media: &MediaInfo, plan: &TranscodePlan) -> Option<String> {
    let FpsPolicy::Cfr { fps } = plan.video.fps else { return None };
    let video = media.video.first()?.duration_sec?;
    let audio = plan
        .audio
        .iter()
        .filter_map(|t| media.audio.iter().find(|a| a.index == t.source_index)?.duration_sec)
        .fold(None, |m: Option<f64>, d| Some(m.map_or(d, |m| m.max(d))))?;
    let diff = audio - video;
    (fps > 0.0 && diff > 0.5 / fps).then(|| format!("tpad=stop_mode=clone:stop_duration={diff:.3}"))
}

#[derive(Default)]
struct Filters {
    vf: Option<String>,
    /// `-i` 之前要加的参数（如 OpenCL 设备初始化）
    pre: Vec<String>,
    /// 有值时替换默认的硬解参数：scale_vt 只接受 VideoToolbox 硬件帧
    hwaccel: Option<Vec<String>>,
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn video_filters(plan: &TranscodePlan, v: &VideoStream) -> Filters {
    let vp = &plan.video;
    let dims = target_dimensions(v, vp.resolution);
    let scale_expr = dims
        .map(|d| {
            if d.portrait {
                format!("scale={}:-2:flags=lanczos", d.w)
            } else {
                format!("scale=-2:{}:flags=lanczos", d.h)
            }
        })
        .unwrap_or_default();
    let chain = |parts: Vec<String>| parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(",");

    // tonemap 为空说明没有可用管线，normalize 已退回保留 HDR；这里不再猜测默认值
    if let (HdrAction::Tonemap, Some(pipeline)) = (vp.hdr_action, vp.tonemap) {
        let dv = if v.dolby_vision.is_some() { ":apply_dolbyvision=1" } else { "" };
        return match pipeline {
            ToneMapPipeline::ScaleVt => {
                // scale_vt 只做色彩空间转换与缩放，不做感知色调映射（技术事实文档 5.4），仅作兜底
                let size = dims.map(|d| format!("w={}:h={}:", d.w, d.h)).unwrap_or_default();
                let mut parts =
                    vec![format!("scale_vt={size}color_matrix=bt709:color_primaries=bt709:color_transfer=bt709")];
                if !vp.encoder.is_hardware() {
                    parts.extend(strings(&["hwdownload", "format=nv12"]));
                }
                Filters {
                    vf: Some(parts.join(",")),
                    pre: Vec::new(),
                    hwaccel: Some(strings(&["-hwaccel", "videotoolbox", "-hwaccel_output_format", "videotoolbox_vld"])),
                }
            }
            ToneMapPipeline::Libplacebo => {
                let size = dims.map(|d| format!("w={}:h={}:downscaler=mitchell:", d.w, d.h)).unwrap_or_default();
                Filters {
                    vf: Some(format!(
                        "libplacebo={size}tonemapping=bt.2390:peak_detect=1:gamut_mode=perceptual{dv}\
                         :colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv:format=yuv420p"
                    )),
                    ..Default::default()
                }
            }
            ToneMapPipeline::TonemapOpencl => Filters {
                pre: strings(&["-init_hw_device", "opencl=ocl", "-filter_hw_device", "ocl"]),
                vf: Some(chain(vec![
                    "format=p010".into(),
                    "hwupload".into(),
                    "tonemap_opencl=tonemap=hable:desat=0:t=bt709:m=bt709:p=bt709:r=tv:format=nv12".into(),
                    "hwdownload".into(),
                    "format=nv12".into(),
                    scale_expr,
                ])),
                hwaccel: None,
            },
            ToneMapPipeline::Zscale => Filters {
                vf: Some(chain(vec![
                    "zscale=t=linear:npl=100".into(),
                    "format=gbrpf32le".into(),
                    "zscale=p=bt709".into(),
                    "tonemap=tonemap=hable:desat=0".into(),
                    "zscale=t=bt709:m=bt709:r=tv".into(),
                    "format=yuv420p".into(),
                    scale_expr,
                ])),
                ..Default::default()
            },
        };
    }
    Filters { vf: (!scale_expr.is_empty()).then_some(scale_expr), ..Default::default() }
}

fn encoder_args(plan: &TranscodePlan, v: &VideoStream) -> Vec<String> {
    use EncoderId::*;
    let vp = &plan.video;
    let ten = vp.bit_depth == 10;
    let q = vp.quality_value.to_string();
    let mut out = vec!["-c:v".to_string(), vp.encoder.name().to_string()];
    let mut push = |items: &[&str]| out.extend(items.iter().map(|s| s.to_string()));

    match vp.encoder {
        Libx265 => {
            push(&["-pix_fmt", if ten { "yuv420p10le" } else { "yuv420p" }, "-preset", &vp.preset, "-crf", &q]);
            let mut params = vec!["repeat-headers=1".to_string()];
            // HDR10 元数据由 ffmpeg 自动透传，这里只打开码率分配优化，不手写 master-display
            if vp.hdr_action == HdrAction::Keep && v.color.hdr_kind == crate::model::HdrKind::Hdr10 {
                params.insert(0, "hdr10-opt=1".into());
            }
            if let Some(extra) = vp.extra_params.as_deref().filter(|s| !s.is_empty()) {
                params.push(extra.to_string());
            }
            push(&["-x265-params", &params.join(":")]);
        }
        Libx264 => {
            push(&[
                "-pix_fmt",
                if ten { "yuv420p10le" } else { "yuv420p" },
                "-profile:v",
                if ten { "high10" } else { "high" },
            ]);
            push(&["-preset", &vp.preset, "-crf", &q]);
        }
        Libsvtav1 => {
            push(&["-pix_fmt", if ten { "yuv420p10le" } else { "yuv420p" }, "-preset", &vp.preset, "-crf", &q]);
            push(&["-svtav1-params", "tune=0"]);
        }
        HevcQsv | H264Qsv | Av1Qsv => {
            // QSV 10bit 必须 p010le；7.0 起默认 RC 变为 CQP，这里用 global_quality 显式指定
            push(&["-pix_fmt", if ten { "p010le" } else { "nv12" }]);
            if ten && vp.encoder == HevcQsv {
                push(&["-profile:v", "main10"]);
            }
            push(&["-preset", &vp.preset, "-global_quality", &q]);
        }
        HevcNvenc | H264Nvenc | Av1Nvenc => {
            push(&["-pix_fmt", if ten { "p010le" } else { "yuv420p" }]);
            if ten && vp.encoder == HevcNvenc {
                push(&["-profile:v", "main10"]);
            }
            // -cq 必须配 -rc vbr -b:v 0，否则会被码率约束
            push(&["-preset", &vp.preset, "-tune", "hq", "-rc", "vbr", "-b:v", "0", "-cq", &q]);
        }
        HevcAmf | H264Amf | Av1Amf => {
            // 显式给像素格式：不支持的格式会被静默换掉（技术事实文档 7.6）
            push(&["-pix_fmt", if ten { "p010le" } else { "nv12" }]);
            push(&["-quality", &vp.preset, "-rc", "cqp", "-qp_i", &q, "-qp_p", &q]);
        }
        HevcVideotoolbox | H264Videotoolbox => {
            if ten && vp.encoder == HevcVideotoolbox {
                push(&["-pix_fmt", "p010le", "-profile:v", "main10"]);
            }
            push(&["-q:v", &q]);
        }
    }

    if let Some(gop) = vp.gop {
        push(&["-g", &gop.to_string()]);
    }
    // 源含杜比视界时必须显式表态：留空会被 auto 自动开启（技术事实文档 3.1）
    if v.dolby_vision.is_some() && matches!(vp.encoder, Libx265 | Libsvtav1) {
        push(&["-dolbyvision", if vp.dovi == DoviAction::Preserve { "1" } else { "0" }]);
    }
    // 不写 -color_primaries / -color_trc：9.0 起这两个输出选项不生效，编码器取帧上的色彩属性
    // （技术事实文档 12 节）。保留 HDR 时解码出的帧自带标签；转 SDR 时由色调映射滤镜打 BT.709 标签

    // 支持引号：-metadata title="My Video" 应是两个 argv，而不是按空白拆成三段
    if let Some(extra) = vp.extra_args.as_deref() {
        out.extend(split_args(extra));
    }
    out
}

/// 把用户输入的附加参数拆成 argv（与前端 `splitArgs` 一致）：单引号完全字面，双引号内只认 `\"` 与 `\\`，
/// 引号外的反斜杠保持字面（Windows 路径不受影响），未闭合的引号宽松处理到结尾
pub fn split_args(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else if q == '"' && c == '\\' && matches!(chars.get(i + 1), Some('"') | Some('\\')) {
                i += 1;
                cur.push(chars[i]);
            } else {
                cur.push(c);
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
            in_token = true;
        } else if c.is_whitespace() {
            if in_token {
                out.push(std::mem::take(&mut cur));
            }
            in_token = false;
        } else {
            cur.push(c);
            in_token = true;
        }
        i += 1;
    }
    if in_token {
        out.push(cur);
    }
    out
}

fn seg(label: &str, args: Vec<String>) -> ArgSegment {
    ArgSegment { label: label.to_string(), args }
}

/// 生成完整命令（分段）。`output` 是实际写入的路径（通常是 `.vidforge-part` 临时文件）；`_caps` 预留给
/// 依赖环境的写法（例如将来按平台选择硬解方式），现有规则都已体现在计划里
pub fn build_arg_segments(
    media: &MediaInfo,
    plan: &TranscodePlan,
    _caps: &Capabilities,
    output: &Path,
) -> Vec<ArgSegment> {
    let v = media.video.first();
    let vp = &plan.video;
    let mut filters = match v {
        Some(v) if vp.action == StreamAction::Encode => video_filters(plan, v),
        _ => Filters::default(),
    };
    // scale_vt 接硬件编码器时帧一直留在 GPU 上，tpad 处理不了，不补
    let hw_frames_to_encoder = filters.hwaccel.is_some() && vp.encoder.is_hardware();
    if let (Some(pad), false) = (tail_pad(media, plan), hw_frames_to_encoder) {
        filters.vf = Some(match filters.vf.take() {
            Some(vf) => format!("{vf},{pad}"),
            None => pad,
        });
    }
    let mut segs = Vec::new();

    // -y：输出是应用自己管理的临时文件，上次中断留下的同名文件直接覆盖；与目标文件的冲突在改名那一步处理
    segs.push(seg(
        "全局",
        strings(&[
            "ffmpeg",
            "-hide_banner",
            "-nostdin",
            "-y",
            "-loglevel",
            "warning",
            "-progress",
            "pipe:1",
            "-nostats",
        ]),
    ));

    let mut input = filters.hwaccel.clone().unwrap_or_else(|| hwaccel_for(plan, media));
    input.extend(filters.pre.iter().cloned());
    input.extend(["-i".to_string(), media.path.clone()]);
    segs.push(seg("输入", input));

    // 映射
    // 按分析得到的流序号映射：`0:v:0` 会把排在前面的封面图也算进去
    let mut map = vec!["-map".to_string(), format!("0:{}", v.map_or(0, |v| v.index))];
    for t in &plan.audio {
        map.extend(["-map".to_string(), format!("0:{}", t.source_index)]);
    }
    let text_subs = || media.subtitle.iter().filter(|s| !s.image_based);
    match plan.subtitles {
        SubtitleMode::All if !media.subtitle.is_empty() => {
            if plan.container == Container::Mkv {
                map.extend(strings(&["-map", "0:s?"]));
            } else {
                for s in text_subs() {
                    map.extend(["-map".to_string(), format!("0:{}", s.index)]);
                }
            }
        }
        SubtitleMode::TextOnly => {
            for s in text_subs() {
                map.extend(["-map".to_string(), format!("0:{}", s.index)]);
            }
        }
        _ => {}
    }
    // MKV 附件多是 ASS 字幕要用的字体，保留字幕时一起带上（MP4 装不下附件）
    if plan.container == Container::Mkv && plan.subtitles != SubtitleMode::None && media.attachments > 0 {
        map.extend(strings(&["-map", "0:t?"]));
    }
    if media.chapters > 0 {
        map.extend(strings(&["-map_chapters", "0"]));
    }
    map.extend(strings(&["-map_metadata", "0"]));
    segs.push(seg("映射", map));

    // 视频
    match v {
        Some(v) if vp.action == StreamAction::Encode => {
            segs.push(seg("视频", encoder_args(plan, v)));
            if let Some(vf) = &filters.vf {
                segs.push(seg("滤镜", vec!["-vf".into(), vf.clone()]));
            }
            // 实测：fps 滤镜会丢最后一帧，-vsync 在 9.0 已移除，唯一正确写法是 -fps_mode:v cfr 加 -r
            match vp.fps {
                FpsPolicy::Cfr { fps } => {
                    segs.push(seg("帧率", vec!["-fps_mode:v".into(), "cfr".into(), "-r".into(), fps_arg(fps)]))
                }
                FpsPolicy::Cap { max } if v.fps_nominal > max => {
                    segs.push(seg("帧率", vec!["-fps_mode:v".into(), "cfr".into(), "-r".into(), fps_arg(max)]));
                }
                _ => {}
            }
        }
        _ => segs.push(seg("视频", strings(&["-c:v", "copy"]))),
    }

    // 音频
    let mut audio = Vec::new();
    for (i, t) in plan.audio.iter().enumerate() {
        let src = media.audio.iter().find(|a| a.index == t.source_index);
        if t.action == StreamAction::Copy {
            audio.extend([format!("-c:a:{i}"), "copy".into()]);
        } else {
            audio.extend([format!("-c:a:{i}"), t.codec.map(|c| c.name()).unwrap_or("aac").to_string()]);
            if let Some(kbps) = t.bitrate_kbps {
                audio.extend([format!("-b:a:{i}"), format!("{kbps}k")]);
            }
            let mut af = Vec::new();
            let use_pan = src.is_some_and(|s| t.channels == Some(2) && s.channels > 2);
            if let (true, Some(s)) = (use_pan, src) {
                af.push(downmix_filter(s));
            }
            if matches!(vp.fps, FpsPolicy::Cfr { .. }) {
                af.push("aresample=async=1".to_string());
            }
            if !af.is_empty() {
                audio.extend([format!("-filter:a:{i}"), af.join(",")]);
            }
            // pan 已确定声道布局，之后不能再加 -ac，否则会被二次重混
            if let (false, Some(ch), Some(s)) = (use_pan, t.channels, src) {
                if s.channels != ch {
                    audio.extend([format!("-ac:a:{i}"), ch.to_string()]);
                }
            }
        }
        if let Some(title) = &t.title {
            audio.extend([format!("-metadata:s:a:{i}"), format!("title={title}")]);
        }
    }
    if !audio.is_empty() {
        segs.push(seg("音频", audio));
    }

    // 字幕
    let has_subs = match plan.subtitles {
        SubtitleMode::All => !media.subtitle.is_empty(),
        SubtitleMode::TextOnly => text_subs().next().is_some(),
        SubtitleMode::None => false,
    };
    if has_subs {
        segs.push(seg(
            "字幕",
            vec!["-c:s".into(), if plan.container == Container::Mkv { "copy" } else { "mov_text" }.into()],
        ));
    }

    // 容器
    let mut mux = Vec::new();
    if plan.container != Container::Mkv {
        let hevc_out = if vp.action == StreamAction::Copy {
            v.is_some_and(|v| v.codec == "hevc")
        } else {
            vp.codec == crate::model::Codec::Hevc
        };
        if hevc_out {
            mux.extend(strings(&["-tag:v", "hvc1"])); // Apple 全家只认 hvc1，ffmpeg 默认写 hev1
        }
        let dv_out = if vp.action == StreamAction::Copy {
            v.is_some_and(|v| v.dolby_vision.is_some())
        } else {
            vp.dovi == DoviAction::Preserve
        };
        if dv_out {
            mux.extend(strings(&["-strict", "unofficial"])); // 否则 dvcC/dvvC box 不写入
        }
        mux.extend(strings(&["-movflags", "+faststart"]));
    }
    // 实际写入 .vidforge-part 临时文件，扩展名无法推断格式，必须显式 -f
    mux.extend(["-f".to_string(), plan.container.muxer().to_string()]);
    segs.push(seg("封装", mux));

    segs.push(seg("输出", vec![output.to_string_lossy().to_string()]));
    segs
}

pub fn flatten(segs: &[ArgSegment]) -> Vec<String> {
    segs.iter().flat_map(|s| s.args.iter().cloned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_args_matches_frontend_rules() {
        assert_eq!(split_args(r#"-metadata title="My Video""#), ["-metadata", "title=My Video"]);
        assert_eq!(split_args("-metadata 'title=My Video'"), ["-metadata", "title=My Video"]);
        assert_eq!(split_args("  -tune   grain  "), ["-tune", "grain"]);
        assert_eq!(split_args(r"-i C:\sub\a.srt"), ["-i", r"C:\sub\a.srt"]);
        assert_eq!(split_args(r#""say \"hi\"" "a\\b""#), [r#"say "hi""#, r"a\b"]);
        assert_eq!(split_args(r#"'a\"b'"#), [r#"a\"b"#]);
        assert_eq!(split_args(r#"-x """#), ["-x", ""]);
        assert!(split_args("   ").is_empty());
    }
}
