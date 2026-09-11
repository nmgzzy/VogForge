//! 媒体分析：ffprobe 输出 → [`MediaInfo`]（设计文档 3.1、5.2）。
//!
//! 每个文件调用三次 ffprobe：
//! 1. `-show_format -show_streams -show_chapters`：流、容器与章节
//! 2. 首帧的 side data：HDR10 静态元数据、HDR10+、杜比视界 RPU（逐帧信息只在帧里）
//! 3. 前 120 个视频包的时间戳：VFR 的第二级判据
//!
//! 解析全部集中在纯函数 [`parse_media`]，测试直接喂真实采集的 fixture。

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::model::{
    AudioStream, ColorInfo, ColorRange, DoviInfo, ElType, Hdr10Metadata, HdrKind, MasteringPrimaries, MediaInfo,
    SourceHint, SubtitleStream, VideoStream,
};

use super::classify::key_line;
use super::exec::{Runner, args};

const INFO_TIMEOUT: Duration = Duration::from_secs(60);
/// VFR 第二级判据的采样包数
pub const VFR_SAMPLE_PACKETS: usize = 120;

/// ffprobe 的三份原始输出
#[derive(Debug, Clone, Default)]
pub struct ProbeOutputs {
    pub info_json: String,
    pub frame_json: String,
    pub packets_csv: String,
}

/// 调 ffprobe 分析一个文件。失败时返回给用户看的中文原因。
pub fn probe_file(ffprobe: &Path, path: &Path, runner: &dyn Runner) -> Result<MediaInfo, String> {
    let file = path.to_string_lossy().to_string();
    let run = |a: Vec<String>| runner.run(ffprobe, &a, INFO_TIMEOUT);

    let info = run(args(["-v", "error", "-show_format", "-show_streams", "-show_chapters", "-of", "json", &file]))
        .map_err(|e| format!("无法运行 ffprobe：{e}"))?;
    if info.timed_out {
        return Err("分析超时（60 秒），文件可能在很慢的网络盘上".to_string());
    }
    if !info.success() {
        return Err(explain_probe_error(&info.stderr));
    }

    let mut outs = ProbeOutputs { info_json: info.stdout, ..Default::default() };
    // 后两次只为补充细节，失败不影响导入。按流序号选主视频流：`v:0` 可能选中排在前面的封面图
    let main = serde_json::from_str::<Value>(&outs.info_json).ok().and_then(|v| main_video_index(&v));
    if let Some(index) = main {
        let select = index.to_string();
        let frame = run(args([
            "-v",
            "error",
            "-select_streams",
            &select,
            "-read_intervals",
            "%+#1",
            "-show_frames",
            "-show_entries",
            "frame=side_data_list,color_range,color_space,color_primaries,color_transfer",
            "-of",
            "json",
            &file,
        ]));
        outs.frame_json = frame.map(|o| o.stdout).unwrap_or_default();
        let packets = run(args([
            "-v",
            "error",
            "-select_streams",
            &select,
            "-read_intervals",
            &format!("%+#{VFR_SAMPLE_PACKETS}"),
            "-show_packets",
            "-show_entries",
            "packet=pts_time",
            "-of",
            "csv=p=0",
            &file,
        ]));
        outs.packets_csv = packets.map(|o| o.stdout).unwrap_or_default();
    }

    let size = std::fs::metadata(path).map(|m| m.len()).ok();
    parse_media(&file, size, &outs)
}

/// 是否 MP4 封面图之类的"附带图片"视频流
fn is_attached_pic(s: &Value) -> bool {
    num(&s["disposition"]["attached_pic"]).unwrap_or(0.0) > 0.0
}

/// 第一条真正的视频流（跳过封面图）的流序号
pub fn main_video_index(info: &Value) -> Option<u64> {
    info["streams"]
        .as_array()?
        .iter()
        .find(|s| s["codec_type"] == "video" && !is_attached_pic(s))
        .and_then(|s| s["index"].as_u64())
}

/// 把 ffprobe 的报错翻译成用户能理解的原因，原文保留在括号里
pub fn explain_probe_error(stderr: &str) -> String {
    let s = stderr.to_ascii_lowercase();
    let raw = key_line(stderr);
    let why = if s.contains("moov atom not found") {
        "文件不完整或已损坏，常见于拍摄中断、复制未完成"
    } else if s.contains("no such file") {
        "文件不存在"
    } else if s.contains("permission denied") {
        "没有读取权限"
    } else if s.contains("invalid data found when processing input") {
        "不是可识别的媒体文件"
    } else {
        "ffprobe 无法分析这个文件"
    };
    if raw.is_empty() { why.to_string() } else { format!("{why}（{raw}）") }
}

// ───────────────────────── 纯解析 ─────────────────────────

/// `"30000/1001"` → 29.97；`"0/0"`、`"N/A"` → None
pub fn rational(s: &str) -> Option<f64> {
    let s = s.trim();
    let v = match s.split_once('/') {
        Some((a, b)) => {
            let (a, b): (f64, f64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
            if b == 0.0 {
                return None;
            }
            a / b
        }
        None => s.parse().ok()?,
    };
    (v.is_finite()).then_some(v)
}

/// JSON 值可能是数字也可能是字符串（ffprobe 两种都用）
fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => rational(s),
        _ => None,
    }
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty() && *s != "N/A").map(String::from)
}

/// 标签查找：大小写不敏感，并兼容 MKV 统计标签的语言后缀（`BPS-eng`、`NUMBER_OF_FRAMES-eng`）
fn tag(obj: &Value, key: &str) -> Option<String> {
    let tags = obj["tags"].as_object()?;
    let key = key.to_ascii_lowercase();
    tags.iter()
        .find(|(k, _)| {
            let k = k.to_ascii_lowercase();
            k == key || k.strip_prefix(&key).is_some_and(|rest| rest.starts_with('-'))
        })
        .and_then(|(_, v)| text(v))
}

/// MKV 的 `DURATION` 标签：`02:25:53.745000000`
fn parse_hms(s: &str) -> Option<f64> {
    let mut parts = s.split(':').rev();
    let sec: f64 = parts.next()?.parse().ok()?;
    let min: f64 = parts.next().map(|p| p.parse().ok()).unwrap_or(Some(0.0))?;
    let hour: f64 = parts.next().map(|p| p.parse().ok()).unwrap_or(Some(0.0))?;
    Some(hour * 3600.0 + min * 60.0 + sec)
}

fn side_data<'a>(list: &'a Value, kind: &str) -> Option<&'a Value> {
    list.as_array()?.iter().find(|sd| sd["side_data_type"].as_str().is_some_and(|t| t.contains(kind)))
}

/// 在 JSON 里递归找某个键（ffprobe 对杜比视界元数据的嵌套层次随版本变化）
fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => map.get(key).or_else(|| map.values().find_map(|x| find_key(x, key))),
        Value::Array(arr) => arr.iter().find_map(|x| find_key(x, key)),
        _ => None,
    }
}

fn bit_depth(pix_fmt: &str, bits_per_raw_sample: Option<f64>) -> u8 {
    let p = pix_fmt.to_ascii_lowercase();
    if p.contains("12le") || p.contains("12be") || p.contains("p012") || p.contains("p016") || p.contains("16le") {
        12
    } else if p.contains("10le") || p.contains("10be") || p.contains("p010") || p.contains("p210") || p.contains("y210")
    {
        10
    } else {
        match bits_per_raw_sample {
            Some(b) if b >= 12.0 => 12,
            Some(b) if b >= 10.0 => 10,
            _ => 8,
        }
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.005
}

/// 按母版显示器的红绿三基色坐标判断色域
fn classify_primaries(md: &Value) -> MasteringPrimaries {
    let get = |k: &str| num(&md[k]);
    match (get("red_x"), get("red_y"), get("green_x"), get("green_y")) {
        (Some(rx), Some(ry), Some(gx), Some(gy)) => {
            if close(rx, 0.680) && close(ry, 0.320) && close(gx, 0.265) && close(gy, 0.690) {
                MasteringPrimaries::P3
            } else if close(rx, 0.708) && close(ry, 0.292) && close(gx, 0.170) && close(gy, 0.797) {
                MasteringPrimaries::Bt2020
            } else {
                MasteringPrimaries::Unknown
            }
        }
        _ => MasteringPrimaries::Unknown,
    }
}

/// HDR10 静态元数据：优先首帧（码流里的 SEI / OBU），其次流级（容器里的 mdcv/clli 或 MKV Colour）。
/// 有理数一律求值成浮点：HEVC 是 `10000000/10000`，AV1 帧级是 `256000/256`，AV1 流级还会被约分成 `1000/1`。
fn hdr10_metadata(stream_sd: &Value, frame_sd: &Value) -> Option<Hdr10Metadata> {
    let md = side_data(frame_sd, "Mastering display metadata")
        .or_else(|| side_data(stream_sd, "Mastering display metadata"));
    let cll = side_data(frame_sd, "Content light level").or_else(|| side_data(stream_sd, "Content light level"));
    let md = md?;
    let max_luminance = num(&md["max_luminance"])?;
    Some(Hdr10Metadata {
        max_luminance,
        min_luminance: num(&md["min_luminance"]).unwrap_or(0.0),
        max_cll: cll.and_then(|c| num(&c["max_content"])).filter(|v| *v > 0.0),
        max_fall: cll.and_then(|c| num(&c["max_average"])).filter(|v| *v > 0.0),
        mastering_primaries: classify_primaries(md),
    })
}

fn dolby_vision(stream_sd: &Value, frame_sd: &Value) -> Option<DoviInfo> {
    let conf = side_data(stream_sd, "DOVI configuration record")?;
    let profile = num(&conf["dv_profile"])? as u8;
    let has_el = num(&conf["el_present_flag"]).unwrap_or(0.0) > 0.0;
    // 增强层类型只能从 RPU 看：disable_residual_flag = 1 是 MEL，0 是 FEL
    let el_type = if has_el {
        side_data(frame_sd, "Dolby Vision Metadata")
            .and_then(|m| find_key(m, "disable_residual_flag"))
            .and_then(num)
            .map(|f| if f > 0.0 { ElType::Mel } else { ElType::Fel })
    } else {
        None
    };
    Some(DoviInfo {
        profile,
        bl_compat_id: num(&conf["dv_bl_signal_compatibility_id"]).unwrap_or(0.0) as u8,
        has_enhancement_layer: has_el,
        el_type,
    })
}

/// VFR 第二级判据（设计文档 5.2）：按时间戳排序后看包间隔。
///
/// 不能用 `duration_time`：MKV 的每帧时长取自 `default_duration`，对可变帧率内容也是常数（技术事实文档 4.2 节）。
/// 容器时间戳有取整（MKV 是毫秒），23.976 固定帧率的间隔会在 41/42 ms 之间跳，所以用"偏离中位数 20% 以上"
/// 判定异常间隔，且至少 2 个、占比 2% 以上才算，避免个别丢帧误报。
pub fn pts_is_vfr(pts: &[f64]) -> bool {
    let mut p: Vec<f64> = pts.iter().copied().filter(|v| v.is_finite()).collect();
    p.sort_by(f64::total_cmp);
    let deltas: Vec<f64> = p.windows(2).map(|w| w[1] - w[0]).filter(|d| *d > 1e-6).collect();
    if deltas.len() < 10 {
        return false;
    }
    let mut sorted = deltas.clone();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    let outliers = deltas.iter().filter(|d| ((*d - median) / median).abs() > 0.2).count();
    outliers >= 2 && outliers as f64 >= deltas.len() as f64 * 0.02
}

fn parse_packets(csv: &str) -> Vec<f64> {
    csv.lines().filter_map(|l| l.trim().trim_end_matches(',').parse::<f64>().ok()).collect()
}

const LOSSLESS_CODECS: &[&str] = &["truehd", "mlp", "flac", "alac", "wavpack", "tta", "ape"];
const IMAGE_SUBTITLES: &[&str] = &["hdmv_pgs_subtitle", "dvd_subtitle", "dvb_subtitle", "xsub"];

fn audio_stream(s: &Value) -> AudioStream {
    let codec = text(&s["codec_name"]).unwrap_or_else(|| "unknown".into());
    let profile = text(&s["profile"]);
    let prof = profile.clone().unwrap_or_default();
    let channels = num(&s["channels"]).unwrap_or(0.0) as u32;
    let lossless = LOSSLESS_CODECS.contains(&codec.as_str())
        || codec.starts_with("pcm_")
        || (codec == "dts" && prof.contains("DTS-HD MA"));
    AudioStream {
        index: num(&s["index"]).unwrap_or(0.0) as u32,
        channel_layout: text(&s["channel_layout"]).unwrap_or_else(|| match channels {
            1 => "mono".into(),
            2 => "stereo".into(),
            n => format!("{n} channels"),
        }),
        channels,
        sample_rate: num(&s["sample_rate"]).unwrap_or(0.0) as u32,
        bitrate: num(&s["bit_rate"]).or_else(|| tag(s, "BPS").and_then(|b| rational(&b))).map(|b| b as u64),
        language: tag(s, "language"),
        title: tag(s, "title"),
        is_default: num(&s["disposition"]["default"]).unwrap_or(0.0) > 0.0,
        lossless,
        atmos: prof.contains("Atmos"),
        dts_x: prof.contains("DTS:X"),
        profile,
        codec,
    }
}

fn subtitle_stream(s: &Value) -> SubtitleStream {
    let codec = text(&s["codec_name"]).unwrap_or_else(|| "unknown".into());
    SubtitleStream {
        index: num(&s["index"]).unwrap_or(0.0) as u32,
        image_based: IMAGE_SUBTITLES.contains(&codec.as_str()),
        language: tag(s, "language"),
        title: tag(s, "title"),
        codec,
    }
}

fn video_stream(s: &Value, first_frame: &Value, packets: &[f64], is_first: bool) -> VideoStream {
    let stream_sd = &s["side_data_list"];
    let pix_fmt = text(&s["pix_fmt"]).unwrap_or_else(|| "unknown".into());
    let fps_nominal = num(&s["r_frame_rate"]).unwrap_or(0.0);
    let fps_avg = num(&s["avg_frame_rate"]).filter(|v| *v > 0.0).unwrap_or(fps_nominal);
    // 只有第一条视频流才采样了帧与包，其余流只用字段判据
    let frame = if is_first { first_frame } else { &Value::Null };
    let frame_sd = &frame["side_data_list"];
    // 色彩标签只写在码流 VUI 里、容器没记录时，流级字段是 unknown，要看解码出的首帧（技术事实文档 12 节）
    let color = |key: &str| {
        let known = |v: &Value| text(v).filter(|t| t != "unknown");
        known(&s[key]).or_else(|| known(&frame[key])).unwrap_or_else(|| "unknown".into())
    };
    let vfr_by_fields = fps_nominal > 0.0 && fps_avg > 0.0 && ((fps_nominal - fps_avg) / fps_nominal).abs() > 0.01;
    let vfr_by_pts = is_first && pts_is_vfr(packets);

    let transfer = color("color_transfer");
    let hdr10 = hdr10_metadata(stream_sd, frame_sd);
    let hdr_kind = match transfer.as_str() {
        "smpte2084" if hdr10.is_some() => HdrKind::Hdr10,
        "smpte2084" => HdrKind::PqNoMeta,
        "arib-std-b67" => HdrKind::Hlg,
        _ => HdrKind::None,
    };
    let rotation = side_data(stream_sd, "Display Matrix")
        .and_then(|d| num(&d["rotation"]))
        .or_else(|| tag(s, "rotate").and_then(|r| rational(&r)))
        .map(|r| {
            let r = r.round() as i32 % 360;
            if r > 180 {
                r - 360
            } else if r <= -180 {
                r + 360
            } else {
                r
            }
        })
        .unwrap_or(0);
    let hdr10plus = frame_sd.as_array().is_some_and(|list| {
        list.iter()
            .any(|sd| sd["side_data_type"].as_str().is_some_and(|t| t.contains("SMPTE2094-40") || t.contains("HDR10+")))
    });

    VideoStream {
        index: num(&s["index"]).unwrap_or(0.0) as u32,
        codec: text(&s["codec_name"]).unwrap_or_else(|| "unknown".into()),
        profile: text(&s["profile"]),
        width: num(&s["width"]).unwrap_or(0.0) as u32,
        height: num(&s["height"]).unwrap_or(0.0) as u32,
        fps_avg,
        fps_nominal,
        is_vfr: vfr_by_fields || vfr_by_pts,
        bit_depth: bit_depth(&pix_fmt, num(&s["bits_per_raw_sample"])),
        pix_fmt,
        bitrate: num(&s["bit_rate"]).or_else(|| tag(s, "BPS").and_then(|b| rational(&b))).map(|b| b as u64),
        color: ColorInfo {
            primaries: color("color_primaries"),
            transfer,
            space: color("color_space"),
            range: if color("color_range") == "pc" { ColorRange::Pc } else { ColorRange::Tv },
            hdr_kind,
        },
        hdr10,
        dolby_vision: dolby_vision(stream_sd, frame_sd),
        hdr10plus,
        rotation,
        frame_count: num(&s["nb_frames"])
            .or_else(|| tag(s, "NUMBER_OF_FRAMES").and_then(|n| rational(&n)))
            .filter(|n| *n > 0.0)
            .map(|n| n as u64),
    }
}

const KNOWN_CONTAINERS: &[&str] =
    &["mp4", "m4v", "mov", "mkv", "webm", "avi", "ts", "m2ts", "mts", "mxf", "wmv", "flv", "3gp", "mpg", "mpeg", "vob"];

fn container_name(path: &str, format_name: &str) -> String {
    let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if KNOWN_CONTAINERS.contains(&ext.as_str()) {
        return ext;
    }
    match format_name.split(',').next().unwrap_or("") {
        "matroska" => "mkv".into(),
        "mov" => "mp4".into(),
        "mpegts" => "ts".into(),
        other if !other.is_empty() => other.into(),
        _ => "unknown".into(),
    }
}

/// 由规范化路径派生稳定 id（FNV-1a 64）。Windows 路径大小写不敏感，统一转小写
pub fn media_id(path: &str) -> String {
    let norm = if cfg!(windows) { path.replace('/', "\\").to_lowercase() } else { path.to_string() };
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in norm.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("m-{h:016x}")
}

const PHONE_MAKERS: &[&str] = &[
    "samsung", "xiaomi", "redmi", "huawei", "honor", "oppo", "vivo", "oneplus", "google", "motorola", "realme",
    "meizu", "nothing", "zte", "nubia", "lenovo", "asus",
];
const CAMERA_MAKERS: &[&str] = &[
    "sony",
    "canon",
    "nikon",
    "panasonic",
    "fujifilm",
    "olympus",
    "om digital",
    "leica",
    "blackmagic",
    "sigma",
    "ricoh",
    "z cam",
    "zcam",
];

/// 推断素材来源与拍摄设备
fn detect_source(
    info: &Value,
    path: &str,
    video: &[VideoStream],
    audio: &[AudioStream],
    subs: &[SubtitleStream],
    container: &str,
) -> (SourceHint, Option<String>) {
    let fmt = &info["format"];
    let make = tag(fmt, "com.apple.quicktime.make")
        .or_else(|| tag(fmt, "com.android.manufacturer"))
        .or_else(|| tag(fmt, "make"));
    let model =
        tag(fmt, "com.apple.quicktime.model").or_else(|| tag(fmt, "com.android.model")).or_else(|| tag(fmt, "model"));
    let device = match (&make, &model) {
        (Some(mk), Some(md)) if md.to_lowercase().starts_with(&mk.to_lowercase()) => Some(md.clone()),
        (Some(mk), Some(md)) => Some(format!("{mk} {md}")),
        (Some(mk), None) => Some(mk.clone()),
        (None, Some(md)) => Some(md.clone()),
        (None, None) => None,
    };
    let mk = make.clone().unwrap_or_default().to_lowercase();
    let md = model.clone().unwrap_or_default().to_lowercase();
    let name = Path::new(path).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let has = |needles: &[&str]| needles.iter().any(|n| name.contains(n));

    // 所有流的 handler / encoder 标签，GoPro 与 DJI 常把品牌写在这里
    let mut labels: Vec<String> = [tag(fmt, "encoder"), tag(fmt, "firmware")].into_iter().flatten().collect();
    if let Some(streams) = info["streams"].as_array() {
        for s in streams {
            labels.extend([tag(s, "handler_name"), tag(s, "encoder")].into_iter().flatten());
        }
    }
    let label = labels.join(" ").to_lowercase();

    let v = video.first();
    let screen_fps = v.is_some_and(|v| v.fps_nominal >= 240.0);
    let hint = if has(&["screen", "录屏", "屏幕录制", "rpreplay"]) || (screen_fps && v.is_some_and(|v| v.is_vfr))
    {
        SourceHint::Screen
    } else if mk.contains("apple") || md.contains("iphone") || md.contains("ipad") {
        SourceHint::Iphone
    } else if mk.contains("gopro") || label.contains("gopro") {
        SourceHint::Gopro
    } else if mk.contains("dji") || label.contains("dji") {
        SourceHint::Dji
    } else if tag(fmt, "com.android.version").is_some() || PHONE_MAKERS.iter().any(|p| mk.contains(p)) {
        SourceHint::Android
    } else if CAMERA_MAKERS.iter().any(|c| mk.contains(c)) || container == "mts" {
        SourceHint::Camera
    } else if has(&["bluray", "blu-ray", "bdremux", "bdrip", "remux", "bdmv"])
        || (subs.iter().any(|s| s.codec == "hdmv_pgs_subtitle") && audio.iter().any(|a| a.lossless || a.atmos))
        || container == "m2ts"
    {
        SourceHint::Bluray
    } else if has(&["web-dl", "webdl", "webrip", "web.dl", ".nf.", "amzn", "dsnp", "hmax", "atvp"]) {
        SourceHint::Streaming
    } else {
        SourceHint::Unknown
    };
    (hint, device)
}

/// 把三份 ffprobe 输出组装成 [`MediaInfo`]。`size` 是文件系统报告的大小，ffprobe 没给时用它。
pub fn parse_media(path: &str, size: Option<u64>, outs: &ProbeOutputs) -> Result<MediaInfo, String> {
    let info: Value = serde_json::from_str(&outs.info_json).map_err(|_| "ffprobe 输出无法解析".to_string())?;
    let frame: Value = serde_json::from_str(&outs.frame_json).unwrap_or(Value::Null);
    let first_frame = &frame["frames"][0];
    let packets = parse_packets(&outs.packets_csv);
    let streams = info["streams"].as_array().cloned().unwrap_or_default();
    let fmt = &info["format"];

    let mut video = Vec::new();
    let mut audio = Vec::new();
    let mut subtitle = Vec::new();
    let mut attachments = 0u32;
    for s in &streams {
        match s["codec_type"].as_str() {
            // MP4 的封面图也是一条视频流，不算；首帧与包的采样对应第一条真实视频流（probe_file 按流序号选）
            Some("video") if !is_attached_pic(s) => {
                let first = video.is_empty();
                video.push(video_stream(s, first_frame, &packets, first));
            }
            Some("video") => attachments += 1,
            Some("audio") => audio.push(audio_stream(s)),
            Some("subtitle") => subtitle.push(subtitle_stream(s)),
            Some("attachment") => attachments += 1,
            _ => {}
        }
    }
    if video.is_empty() {
        // 只有音频（或只有封面图）的文件：命令构建以视频为中心，放进来只会得到跑不起来的命令
        return Err(if audio.is_empty() {
            "文件里没有音视频流".to_string()
        } else {
            "文件里没有视频，只有音频。VidForge 只处理视频文件".to_string()
        });
    }

    let stream_duration = streams
        .iter()
        .filter_map(|s| num(&s["duration"]).or_else(|| tag(s, "DURATION").and_then(|d| parse_hms(&d))))
        .fold(0.0_f64, f64::max);
    let duration_sec = num(&fmt["duration"]).filter(|d| *d > 0.0).unwrap_or(stream_duration);
    let size_bytes = num(&fmt["size"]).map(|s| s as u64).or(size).unwrap_or(0);
    let bitrate = num(&fmt["bit_rate"])
        .map(|b| b as u64)
        .or_else(|| (duration_sec > 0.0).then(|| (size_bytes as f64 * 8.0 / duration_sec) as u64))
        .unwrap_or(0);
    let container = container_name(path, fmt["format_name"].as_str().unwrap_or(""));
    let (source_hint, device) = detect_source(&info, path, &video, &audio, &subtitle, &container);

    Ok(MediaInfo {
        id: media_id(path),
        path: path.to_string(),
        name: Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string()),
        container,
        duration_sec,
        size_bytes,
        bitrate,
        video,
        audio,
        subtitle,
        chapters: info["chapters"].as_array().map(|c| c.len() as u32).unwrap_or(0),
        attachments,
        source_hint,
        device,
        import_root: None,
    })
}

#[cfg(test)]
#[path = "probe_tests.rs"]
mod tests;
