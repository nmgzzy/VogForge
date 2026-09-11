//! 三层能力探测与缓存（设计文档 4.1）。
//!
//! 1. 编译能力：`-version` `-encoders` `-filters` `-bsfs` `-hwaccels` `-protocols`，以及各编码器的 `-h encoder=`
//! 2. 设备初始化：`-init_hw_device <type>=hw`，退出码 0 才算有设备
//! 3. 真实试编码：`-f lavfi -i testsrc2 -frames:v 3 … -f null -`
//!
//! 编译开关一律按"组件是否真的存在"判断（编码器、滤镜、协议），不看 configuration 行：
//! 自动检测到的库（例如 macOS 上的 VideoToolbox）不会出现在 `--enable-` 列表里。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::{bundled_ffmpeg_dir, write_json_atomic};
use crate::external::probe_external;
use crate::i18n::{Lang, pick};
use crate::model::{
    BuildFlag, Capabilities, DeviceProbe, EncoderId, EncoderProbe, EnvStatus, FailureKind, Platform, ProbeProgress,
    ToneMapPipeline, TonemapBlock, TonemapProbe, Vendor,
};
use crate::sysinfo;
use crate::tr;
use crate::util::{now_iso, par_map};

use super::classify::{classify, key_line};
use super::exec::{ExecOutput, Runner, args};
use super::locate::{Env, LocateOptions, Located, locate};
use super::parse::{
    MIN_VERSION, detect_build_source, help_has_option, parse_codec_list, parse_filters, parse_name_list, parse_pix_fmts,
};

/// 缓存格式版本。探测逻辑变化时加一，旧缓存自动作废（2：色调映射不可用原因改为结构化）
const CACHE_SCHEMA: u32 = 2;
const CACHE_FILE: &str = "capabilities.json";
const WORKERS: usize = 4;
const LIST_TIMEOUT: Duration = Duration::from_secs(15);
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct ProbeContext<'a> {
    pub env: &'a dyn Env,
    pub runner: &'a dyn Runner,
    pub platform: Platform,
    pub app_dir: PathBuf,
    /// 设置里指定的 ffmpeg 路径
    pub user_path: Option<PathBuf>,
    /// 说明文字的语言
    pub lang: Lang,
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    schema: u32,
    caps: Capabilities,
}

/// 探测各阶段的名字（中文, 英文）
type Stage = (&'static str, &'static str);
const STAGE_LOCATE: Stage = ("查找 ffmpeg", "Locating ffmpeg");
const STAGE_CACHE: Stage = ("读取缓存", "Reading cache");
const STAGE_BUILD: Stage = ("检测编译能力", "Checking build features");
const STAGE_HELP: Stage = ("读取编码器参数", "Reading encoder options");
const STAGE_DEVICES: Stage = ("初始化硬件设备", "Initializing hardware devices");
const STAGE_TEST: Stage = ("试编码", "Test encoding");
const STAGE_TONEMAP: Stage = ("检测色调映射", "Checking tone mapping");

/// 探测入口。`force` 为 false 时，指纹未变就直接返回缓存（外部工具仍会重新查找，开销很小）。
///
/// 缓存里存的是中文原文；返回前按 `ctx.lang` 用 [`localize`] 从结构化字段重新生成说明，切换语言不需要重新探测。
pub fn probe(ctx: &ProbeContext, force: bool, progress: &(dyn Fn(ProbeProgress) + Sync)) -> Capabilities {
    let lang = ctx.lang;
    let report = |stage: Stage, done: usize, total: usize| {
        let stage = pick(lang, stage.0, stage.1).to_string();
        progress(ProbeProgress { stage, done: done as u32, total: total as u32 });
    };
    report(STAGE_LOCATE, 0, 1);

    let opts = LocateOptions {
        user_path: ctx.user_path.clone(),
        bundled_dir: Some(bundled_ffmpeg_dir(&ctx.app_dir)),
        platform: ctx.platform,
        lang,
    };
    let located = locate(&opts, ctx.env, ctx.runner);
    let gpus = sysinfo::gpus(ctx.runner);
    let external = probe_external(ctx.platform, ctx.env, ctx.runner, lang);

    let Some(found) = located.found else {
        // 有 ffmpeg 可执行文件却用不了，与根本没装是两种问题，给用户的建议也不同
        let mut caps = match located.broken.first() {
            Some(reason) => Capabilities::placeholder(
                EnvStatus::Broken,
                tr!(
                    lang,
                    "找到了 ffmpeg，但无法使用：{}。请换一套完整的构建，或在设置里指定其他目录。",
                    "Found ffmpeg, but it cannot be used: {}. Install a complete build, or choose another folder in Settings.",
                    reason
                ),
            ),
            None => Capabilities::placeholder(
                EnvStatus::Missing,
                pick(
                    lang,
                    "没有找到 ffmpeg 与 ffprobe。请安装 ffmpeg 7.1 或更高版本，或在设置里指定它所在的目录。",
                    "ffmpeg and ffprobe were not found. Install ffmpeg 7.1 or newer, or choose its folder in Settings.",
                ),
            ),
        };
        caps.searched = located.searched;
        caps.notes = located.problems;
        caps.gpus = gpus;
        caps.external = external;
        caps.platform = ctx.platform;
        caps.probed_at = now_iso();
        report(STAGE_LOCATE, 1, 1);
        return caps;
    };

    // 与"这次怎么找到的"相关的字段每次都重新填，缓存只负责省掉三层探测
    let notes = locate_notes(&found, located.problems, lang);
    let fingerprint = fingerprint(&found, &gpus);
    if !force {
        if let Some(mut cached) = load_cache(&ctx.app_dir).filter(|c| c.fingerprint == fingerprint) {
            cached.locate_source = Some(found.source);
            cached.searched = located.searched;
            cached.notes = notes;
            cached.external = external;
            report(STAGE_CACHE, 1, 1);
            localize(&mut cached, lang);
            return cached;
        }
    }

    let mut caps = full_probe(ctx, &found, &report);
    caps.searched = located.searched;
    caps.notes = notes;
    caps.gpus = gpus;
    caps.external = external;
    caps.fingerprint = fingerprint;
    caps.probed_at = now_iso();
    let _ = save_cache(&ctx.app_dir, &caps);
    localize(&mut caps, lang);
    caps
}

/// 找到了 ffmpeg 时仍值得告诉用户的事：设置里指定的路径无效、ffprobe 与 ffmpeg 版本不一致。
/// "只有 ffmpeg 没有 ffprobe"已经从其他目录找到了完整的一套，不再提
fn locate_notes(found: &Located, problems: Vec<String>, lang: Lang) -> Vec<String> {
    let lonely = [" 里只有 ffmpeg，没有 ffprobe", " has ffmpeg but no ffprobe"];
    let mut notes: Vec<String> = problems.into_iter().filter(|p| !lonely.iter().any(|l| p.contains(l))).collect();
    if let Some(pv) = &found.ffprobe_version {
        if pv.raw != found.version.raw {
            notes.push(tr!(
                lang,
                "ffprobe 版本（{}）与 ffmpeg（{}）不一致，建议使用同一套构建",
                "The ffprobe version ({}) differs from ffmpeg ({}); use both from the same build",
                pv.raw,
                found.version.raw
            ));
        }
    }
    notes
}

/// 把缓存里的中文说明换成 `lang` 的版本。只改由结构化字段决定的文字；ffmpeg 原文（设备与编码器报错）保持原样
pub fn localize(caps: &mut Capabilities, lang: Lang) {
    if lang == Lang::ZhCn {
        return;
    }
    caps.version_number = caps.version_number.replace("（开发版）", " (dev build)").replace("未知", "unknown");
    caps.build_source = match caps.build_source.as_str() {
        "Linux 发行版" => "Linux distribution".into(),
        "未知来源" => "unknown".into(),
        other => other.into(),
    };
    if caps.status == EnvStatus::TooOld {
        caps.status_detail = too_old_detail(&caps.version_number, lang);
    }
    for f in &mut caps.build_flags {
        if let Some(affects) = flag_affects(&f.name, lang) {
            f.affects = affects.into();
        }
    }
    for e in &mut caps.encoders {
        e.error = e.error.take().map(|err| match err.as_str() {
            NOT_BUILT_ZH => pick(lang, NOT_BUILT_ZH, "This ffmpeg was built without this encoder").into(),
            TIMEOUT_ZH => pick(lang, TIMEOUT_ZH, "The test encode timed out").into(),
            _ => err,
        });
    }
    for t in &mut caps.tonemap {
        t.note = tonemap_note(t.id, t.block.as_ref(), lang);
    }
    // 外部工具每次探测都按语言重新生成；这里照顾浏览器预览里固定的示例能力
    for x in &mut caps.external {
        if let Some(p) = crate::external::purpose(&x.name, lang) {
            x.purpose = p.into();
        }
    }
}

const NOT_BUILT_ZH: &str = "当前 ffmpeg 没有编译这个编码器";
const TIMEOUT_ZH: &str = "试编码超时";

fn too_old_detail(version: &str, lang: Lang) -> String {
    tr!(
        lang,
        "ffmpeg {} 低于最低要求 {}。杜比视界保留需要 7.1 起的 libx265 -dolbyvision，旧版本还可能丢失 HDR10 元数据。请升级后再转码。",
        "ffmpeg {} is older than the required {}. Keeping Dolby Vision needs libx265 -dolbyvision from 7.1, and older versions may also drop HDR10 metadata. Upgrade before transcoding.",
        version,
        MIN_VERSION
    )
}

/// 色调映射管线的说明：可用时是特点，不可用时是原因
fn tonemap_note(p: ToneMapPipeline, block: Option<&TonemapBlock>, lang: Lang) -> String {
    match block {
        Some(TonemapBlock::NotBuilt { what }) if what == "scale_vt" => {
            pick(lang, "当前 ffmpeg 没有 scale_vt 滤镜", "This ffmpeg has no scale_vt filter").into()
        }
        Some(TonemapBlock::NotBuilt { what }) => {
            tr!(lang, "当前 ffmpeg 未编译 {}", "This ffmpeg was built without {}", what)
        }
        Some(TonemapBlock::Device { device, error }) => {
            tr!(lang, "{} 设备初始化失败：{}", "The {} device failed to initialize: {}", device, error)
        }
        Some(TonemapBlock::MacOnly) => pick(lang, "仅 macOS 可用", "Only available on macOS").into(),
        Some(TonemapBlock::TrialFailed { error }) => tr!(lang, "试运行失败：{}", "The trial run failed: {}", error),
        None => match p {
            ToneMapPipeline::Libplacebo => pick(
                lang,
                "质量最好，唯一能正确处理杜比视界 Profile 5",
                "Best quality, and the only one that handles Dolby Vision Profile 5 correctly",
            ),
            ToneMapPipeline::TonemapOpencl => pick(
                lang,
                "GPU 加速，带场景自适应峰值检测，输出限 8bit",
                "GPU accelerated with scene-adaptive peak detection; 8-bit output only",
            ),
            ToneMapPipeline::Zscale => pick(lang, "纯 CPU 兜底，速度较慢", "CPU fallback; slower"),
            ToneMapPipeline::ScaleVt => pick(
                lang,
                "只做色彩空间转换，不做感知色调映射，高光可能过曝",
                "Only converts the color space without perceptual tone mapping; highlights may clip",
            ),
        }
        .into(),
    }
}

fn fingerprint(found: &Located, gpus: &[crate::model::GpuInfo]) -> String {
    let (mtime, size) = fs::metadata(&found.ffmpeg)
        .map(|m| {
            let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
            (t.unwrap_or(0), m.len())
        })
        .unwrap_or((0, 0));
    let gpu = gpus.iter().map(|g| format!("{}@{}", g.name, g.driver)).collect::<Vec<_>>().join(",");
    format!("v{CACHE_SCHEMA}|{}|{mtime}|{size}|{}|{gpu}", found.ffmpeg.display(), found.version.raw)
}

fn load_cache(app_dir: &Path) -> Option<Capabilities> {
    let text = fs::read_to_string(app_dir.join(CACHE_FILE)).ok()?;
    let file: CacheFile = serde_json::from_str(&text).ok()?;
    (file.schema == CACHE_SCHEMA).then_some(file.caps)
}

fn save_cache(app_dir: &Path, caps: &Capabilities) -> std::io::Result<()> {
    write_json_atomic(&app_dir.join(CACHE_FILE), &CacheFile { schema: CACHE_SCHEMA, caps: caps.clone() })
}

/// 当前平台需要关心的编码器
pub fn platform_encoders(platform: Platform) -> Vec<EncoderId> {
    EncoderId::ALL
        .into_iter()
        .filter(|e| match (platform, e.vendor()) {
            (_, Vendor::Software) => true,
            (Platform::Macos, v) => v == Vendor::Apple,
            (Platform::Windows, v) => matches!(v, Vendor::Intel | Vendor::Nvidia | Vendor::Amd),
            (Platform::Linux, v) => matches!(v, Vendor::Intel | Vendor::Nvidia),
        })
        .collect()
}

/// 当前平台需要做初始化探测的硬件设备类型
fn platform_devices(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Windows => &["qsv", "cuda", "d3d11va", "d3d12va", "dxva2", "vulkan", "opencl", "amf"],
        Platform::Macos => &["videotoolbox", "vulkan", "opencl"],
        Platform::Linux => &["vaapi", "qsv", "cuda", "vulkan", "opencl"],
    }
}

fn ffmpeg_run(ctx: &ProbeContext, ffmpeg: &Path, a: &[String], timeout: Duration) -> ExecOutput {
    ctx.runner.run(ffmpeg, a, timeout).unwrap_or_else(|e| ExecOutput {
        code: None,
        stderr: format!("无法运行 ffmpeg：{e}"),
        ..Default::default()
    })
}

/// 试编码参数：`(像素格式, 附加参数)`。10bit 的格式必须出现在编码器自报的像素格式列表里，
/// 否则 ffmpeg 会静默换成 8bit 格式继续编码，试编码"成功"却不代表支持 10bit。
pub fn test_encode_format(enc: EncoderId, ten_bit: bool) -> Option<(&'static str, &'static [&'static str])> {
    use EncoderId::*;
    Some(match (enc, ten_bit) {
        (Libx264, false) | (Libx265, false) | (Libsvtav1, false) => ("yuv420p", &[]),
        (Libx264, true) => ("yuv420p10le", &["-profile:v", "high10"]),
        (Libx265, true) => ("yuv420p10le", &["-profile:v", "main10"]),
        (Libsvtav1, true) => ("yuv420p10le", &[]),
        (H264Qsv, false) | (HevcQsv, false) | (Av1Qsv, false) => ("nv12", &[]),
        (H264Qsv, true) => ("p010le", &[]),
        (HevcQsv, true) => ("p010le", &["-profile:v", "main10"]),
        (Av1Qsv, true) => ("p010le", &[]),
        (H264Nvenc, false) | (HevcNvenc, false) | (Av1Nvenc, false) => ("yuv420p", &[]),
        (H264Nvenc, true) => ("p010le", &["-profile:v", "high10"]),
        (HevcNvenc, true) => ("p010le", &["-profile:v", "main10"]),
        (Av1Nvenc, true) => ("p010le", &[]),
        (H264Amf, false) | (HevcAmf, false) | (Av1Amf, false) => ("nv12", &[]),
        (H264Amf, true) | (HevcAmf, true) | (Av1Amf, true) => ("p010le", &[]),
        (H264Videotoolbox, false) | (HevcVideotoolbox, false) => ("nv12", &[]),
        (HevcVideotoolbox, true) => ("p010le", &["-profile:v", "main10"]),
        (H264Videotoolbox, true) => return None,
    })
}

pub fn test_encode_args(enc: EncoderId, ten_bit: bool) -> Option<Vec<String>> {
    let (pix, extra) = test_encode_format(enc, ten_bit)?;
    let mut a = args(["-hide_banner", "-nostdin", "-v", "error", "-f", "lavfi", "-i", "testsrc2=s=1280x720:r=30"]);
    a.extend(args(["-frames:v", "3", "-pix_fmt", pix]));
    a.extend(extra.iter().map(|s| s.to_string()));
    a.extend(args(["-c:v", enc.name(), "-f", "null", "-"]));
    Some(a)
}

fn device_args(kind: &str) -> Vec<String> {
    let mut a = args(["-hide_banner", "-nostdin", "-v", "error", "-init_hw_device"]);
    a.push(format!("{kind}=hw"));
    a.extend(args(["-f", "lavfi", "-i", "nullsrc=s=64x64:d=0.04", "-f", "null", "-"]));
    a
}

/// 色调映射试运行：用带 BT.2020/PQ 标签的 10bit 测试图跑 2 帧
pub fn tonemap_test_args(p: ToneMapPipeline) -> Vec<String> {
    let mut a = args(["-hide_banner", "-nostdin", "-v", "error"]);
    match p {
        ToneMapPipeline::TonemapOpencl => a.extend(args(["-init_hw_device", "opencl=ocl", "-filter_hw_device", "ocl"])),
        ToneMapPipeline::ScaleVt => a.extend(args(["-init_hw_device", "videotoolbox=vt", "-filter_hw_device", "vt"])),
        _ => {}
    }
    a.extend(args([
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=320x180:r=30,format=yuv420p10le,setparams=color_primaries=bt2020:color_trc=smpte2084:colorspace=bt2020nc",
        "-frames:v",
        "2",
        "-vf",
    ]));
    a.push(
        match p {
            ToneMapPipeline::Libplacebo => {
                "libplacebo=tonemapping=bt.2390:colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv:format=yuv420p"
            }
            ToneMapPipeline::TonemapOpencl => {
                "format=p010,hwupload,tonemap_opencl=tonemap=hable:desat=0:t=bt709:m=bt709:p=bt709:r=tv:format=nv12,hwdownload,format=nv12"
            }
            ToneMapPipeline::Zscale => {
                "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p"
            }
            ToneMapPipeline::ScaleVt => {
                "format=p010le,hwupload,scale_vt=color_matrix=bt709:color_primaries=bt709:color_transfer=bt709,hwdownload,format=nv12"
            }
        }
        .to_string(),
    );
    a.extend(args(["-f", "null", "-"]));
    a
}

struct Lists {
    encoders: HashSet<String>,
    audio_encoders: HashSet<String>,
    filters: HashSet<String>,
    bsfs: HashSet<String>,
    hwaccels: Vec<String>,
    protocols: HashSet<String>,
}

fn full_probe(ctx: &ProbeContext, found: &Located, report: &(dyn Fn(Stage, usize, usize) + Sync)) -> Capabilities {
    let ffmpeg = found.ffmpeg.as_path();

    // ── 第 1 层：编译能力 ──
    let list_cmds = ["-encoders", "-filters", "-bsfs", "-hwaccels", "-protocols"];
    report(STAGE_BUILD, 0, list_cmds.len());
    let outs = par_map(
        &list_cmds,
        WORKERS,
        |flag| ffmpeg_run(ctx, ffmpeg, &args(["-hide_banner", flag]), LIST_TIMEOUT).combined(),
        |n| report(STAGE_BUILD, n, list_cmds.len()),
    );
    let codec_list = parse_codec_list(&outs[0]);
    let lists = Lists {
        encoders: codec_list.iter().filter(|e| e.kind == 'V').map(|e| e.name.clone()).collect(),
        audio_encoders: codec_list.iter().filter(|e| e.kind == 'A').map(|e| e.name.clone()).collect(),
        filters: parse_filters(&outs[1]).into_iter().collect(),
        bsfs: parse_name_list(&outs[2]).into_iter().collect(),
        hwaccels: parse_name_list(&outs[3]),
        protocols: parse_name_list(&outs[4]).into_iter().collect(),
    };

    let candidates: Vec<EncoderId> =
        platform_encoders(ctx.platform).into_iter().filter(|e| lists.encoders.contains(e.name())).collect();
    report(STAGE_HELP, 0, candidates.len());
    let helps = par_map(
        &candidates,
        WORKERS,
        |e| {
            ffmpeg_run(ctx, ffmpeg, &args(["-hide_banner", "-h", &format!("encoder={}", e.name())]), LIST_TIMEOUT)
                .combined()
        },
        |n| report(STAGE_HELP, n, candidates.len()),
    );

    // ── 第 2 层：设备初始化 ──
    let device_kinds: Vec<&str> =
        platform_devices(ctx.platform).iter().copied().filter(|d| lists.hwaccels.iter().any(|h| h == d)).collect();
    report(STAGE_DEVICES, 0, device_kinds.len());
    let devices: Vec<DeviceProbe> = par_map(
        &device_kinds,
        WORKERS,
        |kind| {
            let out = ffmpeg_run(ctx, ffmpeg, &device_args(kind), LIST_TIMEOUT);
            DeviceProbe {
                id: kind.to_string(),
                available: out.success(),
                error: (!out.success()).then(|| key_line(&out.stderr)),
            }
        },
        |n| report(STAGE_DEVICES, n, device_kinds.len()),
    );

    // ── 第 3 层：真实试编码 ──
    report(STAGE_TEST, 0, candidates.len());
    let jobs: Vec<(EncoderId, Vec<String>)> =
        candidates.iter().copied().zip(helps.iter().map(|h| parse_pix_fmts(h))).collect();
    let probed: Vec<EncoderProbe> = par_map(
        &jobs,
        WORKERS,
        |(enc, pix_fmts)| probe_encoder(ctx, ffmpeg, *enc, pix_fmts),
        |n| report(STAGE_TEST, n, candidates.len()),
    );
    let encoders: Vec<EncoderProbe> = platform_encoders(ctx.platform)
        .into_iter()
        .map(|id| {
            probed.iter().find(|p| p.id == id).cloned().unwrap_or(EncoderProbe {
                id,
                vendor: id.vendor(),
                codec: id.codec(),
                usable: false,
                ten_bit: false,
                error: Some(NOT_BUILT_ZH.to_string()),
                failure: Some(FailureKind::NotBuilt),
            })
        })
        .collect();

    // ── 色调映射管线 ──
    let pipelines = ToneMapPipeline::ORDER;
    report(STAGE_TONEMAP, 0, pipelines.len());
    let tonemap = par_map(
        &pipelines,
        WORKERS,
        |p| probe_tonemap(ctx, ffmpeg, *p, &lists, &devices),
        |n| report(STAGE_TONEMAP, n, pipelines.len()),
    );

    let x265_help = candidates.iter().position(|e| *e == EncoderId::Libx265).map(|i| helps[i].as_str()).unwrap_or("");
    let version_ok = found.version.meets_minimum();
    let x265_usable = encoders.iter().any(|e| e.id == EncoderId::Libx265 && e.usable);

    // 缓存里的说明一律用中文，返回前再按语言换（localize）
    let (status, status_detail) = if version_ok {
        (EnvStatus::Ready, String::new())
    } else {
        (EnvStatus::TooOld, too_old_detail(&found.version.display_number(), Lang::ZhCn))
    };

    Capabilities {
        status,
        status_detail,
        ffmpeg_path: found.ffmpeg.display().to_string(),
        ffprobe_path: found.ffprobe.display().to_string(),
        locate_source: Some(found.source),
        searched: Vec::new(),
        notes: Vec::new(),
        version: found.version.raw.clone(),
        version_number: found.version.display_number(),
        build_source: detect_build_source(&found.version),
        build_flags: build_flags(ctx.platform, &lists),
        encoders,
        hwaccels: lists.hwaccels.clone(),
        devices,
        tonemap,
        dolby_vision_encode: version_ok && x265_usable && help_has_option(x265_help, "-dolbyvision"),
        dovi_split: lists.bsfs.contains("dovi_split"),
        external: Vec::new(),
        gpus: Vec::new(),
        platform: ctx.platform,
        probed_at: String::new(),
        fingerprint: String::new(),
    }
}

fn probe_encoder(ctx: &ProbeContext, ffmpeg: &Path, enc: EncoderId, pix_fmts: &[String]) -> EncoderProbe {
    let run = |ten: bool| test_encode_args(enc, ten).map(|a| ffmpeg_run(ctx, ffmpeg, &a, TEST_TIMEOUT));
    let base = EncoderProbe {
        id: enc,
        vendor: enc.vendor(),
        codec: enc.codec(),
        usable: false,
        ten_bit: false,
        error: None,
        failure: None,
    };
    let Some(out8) = run(false) else { return base };
    if !out8.success() {
        let text = if out8.timed_out { TIMEOUT_ZH.to_string() } else { out8.stderr.clone() };
        return EncoderProbe { error: Some(key_line(&text)), failure: Some(classify(&text)), ..base };
    }
    // 10bit：像素格式必须在编码器自报的列表里；列表为空（极旧版本不输出）时仍然尝试
    let ten_fmt = test_encode_format(enc, true).map(|(p, _)| p);
    let listed = ten_fmt.is_some_and(|p| pix_fmts.is_empty() || pix_fmts.iter().any(|f| f == p));
    let ten_bit = listed && run(true).is_some_and(|o| o.success());
    EncoderProbe { usable: true, ten_bit, ..base }
}

fn probe_tonemap(
    ctx: &ProbeContext,
    ffmpeg: &Path,
    p: ToneMapPipeline,
    lists: &Lists,
    devices: &[DeviceProbe],
) -> TonemapProbe {
    let device = |id: &str, label: &str| {
        devices
            .iter()
            .find(|d| d.id == id && !d.available)
            .map(|d| TonemapBlock::Device { device: label.into(), error: d.error.clone().unwrap_or_default() })
    };
    let not_built = |what: &str| Some(TonemapBlock::NotBuilt { what: what.into() });
    let has = |f: &str| lists.filters.contains(f);

    let precheck = match p {
        ToneMapPipeline::Libplacebo if !has("libplacebo") => not_built("libplacebo"),
        ToneMapPipeline::Libplacebo => device("vulkan", "Vulkan"),
        ToneMapPipeline::TonemapOpencl if !has("tonemap_opencl") => not_built("OpenCL"),
        ToneMapPipeline::TonemapOpencl => device("opencl", "OpenCL"),
        ToneMapPipeline::Zscale if !(has("zscale") && has("tonemap")) => not_built("libzimg"),
        ToneMapPipeline::ScaleVt if ctx.platform != Platform::Macos => Some(TonemapBlock::MacOnly),
        ToneMapPipeline::ScaleVt if !has("scale_vt") => not_built("scale_vt"),
        _ => None,
    };
    let block = precheck.or_else(|| {
        let out = ffmpeg_run(ctx, ffmpeg, &tonemap_test_args(p), TEST_TIMEOUT);
        (!out.success()).then(|| TonemapBlock::TrialFailed { error: key_line(&out.stderr) })
    });
    TonemapProbe { id: p, available: block.is_none(), note: tonemap_note(p, block.as_ref(), Lang::ZhCn), block }
}

/// 构建开关缺失时影响的功能（中文, 英文）
const FLAG_AFFECTS: &[(&str, &str, &str)] = &[
    ("libx265", "HEVC 软编、杜比视界保留", "HEVC software encoding, Dolby Vision"),
    ("libx264", "H.264 软编", "H.264 software encoding"),
    ("libsvtav1", "AV1 软编（含 HDR10）", "AV1 software encoding (with HDR10)"),
    ("libplacebo", "最佳色调映射、杜比视界 P5 映射", "Best tone mapping, Dolby Vision P5 mapping"),
    ("vulkan", "libplacebo 运行依赖", "Required by libplacebo"),
    ("opencl", "GPU 色调映射", "GPU tone mapping"),
    ("libzimg", "CPU 色调映射兜底", "CPU tone mapping fallback"),
    ("libvmaf", "画质评分（v2）", "Quality scoring (v2)"),
    ("libbluray", "蓝光原盘读取（v2）", "Blu-ray disc reading (v2)"),
    ("libopus", "Opus 音频编码", "Opus audio encoding"),
    (
        "libfdk_aac",
        "高质量 AAC / HE-AAC（官方构建均不含，已用原生 aac 替代）",
        "High-quality AAC / HE-AAC (absent from official builds; the native aac encoder is used instead)",
    ),
    ("libvpl", "Intel QSV 硬件编码", "Intel QSV hardware encoding"),
    ("nvenc", "NVIDIA 硬件编码", "NVIDIA hardware encoding"),
    ("amf", "AMD 硬件编码", "AMD hardware encoding"),
    ("videotoolbox", "Apple 硬件编码", "Apple hardware encoding"),
];

fn flag_affects(name: &str, lang: Lang) -> Option<&'static str> {
    FLAG_AFFECTS.iter().find(|(n, ..)| *n == name).map(|(_, zh, en)| pick(lang, zh, en))
}

fn build_flags(platform: Platform, l: &Lists) -> Vec<BuildFlag> {
    let enc = |n: &str| l.encoders.contains(n);
    let any_enc = |suffix: &str| l.encoders.iter().any(|e| e.ends_with(suffix));
    let filter = |n: &str| l.filters.contains(n);
    let hw = |n: &str| l.hwaccels.iter().any(|h| h == n);
    let flag = |name: &str, present: bool| BuildFlag {
        name: name.into(),
        present,
        affects: flag_affects(name, Lang::ZhCn).unwrap_or_default().into(),
    };

    let mut flags = vec![
        flag("libx265", enc("libx265")),
        flag("libx264", enc("libx264")),
        flag("libsvtav1", enc("libsvtav1")),
        flag("libplacebo", filter("libplacebo")),
        flag("vulkan", hw("vulkan")),
        flag("opencl", hw("opencl") || filter("tonemap_opencl")),
        flag("libzimg", filter("zscale")),
        flag("libvmaf", filter("libvmaf")),
        flag("libbluray", l.protocols.contains("bluray")),
        flag("libopus", l.audio_encoders.contains("libopus")),
        flag("libfdk_aac", l.audio_encoders.contains("libfdk_aac")),
    ];
    match platform {
        Platform::Windows | Platform::Linux => {
            flags.push(flag("libvpl", any_enc("_qsv")));
            flags.push(flag("nvenc", any_enc("_nvenc")));
            if platform == Platform::Windows {
                flags.push(flag("amf", any_enc("_amf")));
            }
        }
        Platform::Macos => flags.push(flag("videotoolbox", any_enc("_videotoolbox"))),
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffmpeg::locate::SystemEnv;
    use std::collections::HashMap;
    use std::io;
    use std::sync::Mutex;

    const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ffmpeg");

    fn fixture(build: &str, name: &str) -> String {
        fs::read_to_string(format!("{FIX}/{build}/{name}")).unwrap_or_default()
    }

    /// 用真实采集的列表输出模拟一个 ffmpeg；试编码与设备初始化按表返回
    struct FakeFfmpeg {
        build: &'static str,
        /// 失败的试编码：编码器名 → stderr
        encode_fail: HashMap<&'static str, &'static str>,
        /// 10bit 失败的编码器
        ten_fail: HashSet<&'static str>,
        device_fail: HashMap<&'static str, &'static str>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl FakeFfmpeg {
        fn new(build: &'static str) -> Self {
            FakeFfmpeg {
                build,
                encode_fail: HashMap::new(),
                ten_fail: HashSet::new(),
                device_fail: HashMap::new(),
                calls: Mutex::new(Vec::new()),
            }
        }
        fn ok(stdout: String) -> io::Result<ExecOutput> {
            Ok(ExecOutput { code: Some(0), stdout, ..Default::default() })
        }
        fn fail(stderr: &str) -> io::Result<ExecOutput> {
            Ok(ExecOutput { code: Some(1), stderr: stderr.to_string(), ..Default::default() })
        }
    }

    impl Runner for FakeFfmpeg {
        fn run(&self, p: &Path, a: &[String], _t: Duration) -> io::Result<ExecOutput> {
            // 只记 ffmpeg / ffprobe 的调用：macOS 上查显卡还会跑 sw_vers 与 system_profiler
            if p.file_stem().is_some_and(|s| s.to_string_lossy().starts_with("ff")) {
                self.calls.lock().unwrap().push(a.to_vec());
            }
            let has = |x: &str| a.iter().any(|s| s == x);
            if has("-version") {
                return Self::ok(fixture(self.build, "version.txt"));
            }
            for (flag, file) in [
                ("-encoders", "encoders.txt"),
                ("-filters", "filters.txt"),
                ("-bsfs", "bsfs.txt"),
                ("-hwaccels", "hwaccels.txt"),
                ("-protocols", "protocols.txt"),
            ] {
                if has(flag) {
                    return Self::ok(fixture(self.build, file));
                }
            }
            if has("-h") {
                let enc = a.iter().find_map(|s| s.strip_prefix("encoder=")).unwrap_or("");
                let text = if enc == "libx265" { fixture(self.build, "h-libx265.txt") } else { real_pix_fmts(enc) };
                return Self::ok(text);
            }
            if let Some(dev) = a.iter().find_map(|s| s.strip_suffix("=hw")) {
                return match self.device_fail.get(dev) {
                    Some(e) => Self::fail(e),
                    None => Self::ok(String::new()),
                };
            }
            if let Some(i) = a.iter().position(|s| s == "-c:v") {
                let enc = a[i + 1].as_str();
                if let Some(e) = self.encode_fail.get(enc) {
                    return Self::fail(e);
                }
                let ten = a.iter().any(|s| s.contains("10le") || s == "p010le");
                if ten && self.ten_fail.contains(enc) {
                    return Self::fail("10 bit encode not supported");
                }
                return Self::ok(String::new());
            }
            // 色调映射试运行
            Self::ok(String::new())
        }
    }

    /// 本机 `-h encoder=` 实测的像素格式列表
    fn real_pix_fmts(enc: &str) -> String {
        let fmts = match enc {
            "libx264" => "yuv420p yuvj420p yuv422p nv12 yuv420p10le",
            "libsvtav1" => "yuv420p yuv420p10le",
            "h264_qsv" => "nv12 qsv",
            "hevc_qsv" => "nv12 p010le p012le qsv",
            "av1_qsv" => "nv12 p010le qsv",
            _ => "yuv420p nv12 p010le",
        };
        format!("Encoder {enc}:\n    Supported pixel formats: {fmts}\n")
    }

    struct FakeEnv(HashSet<PathBuf>);
    impl Env for FakeEnv {
        fn var(&self, k: &str) -> Option<String> {
            (k == "PATH").then(|| r"C:\ff\bin".to_string())
        }
        fn registry_path_entries(&self) -> Vec<String> {
            Vec::new()
        }
        fn is_file(&self, p: &Path) -> bool {
            self.0.contains(p)
        }
        fn subdirs(&self, _: &Path) -> Vec<PathBuf> {
            Vec::new()
        }
        fn home(&self) -> Option<PathBuf> {
            None
        }
    }

    fn installed_env() -> FakeEnv {
        let dir = PathBuf::from(r"C:\ff\bin");
        FakeEnv([dir.join("ffmpeg.exe"), dir.join("ffprobe.exe")].into_iter().collect())
    }

    const NVENC_ERR: &str = "[h264_nvenc @ 000001f2e4327240] Cannot load nvcuda.dll\r\n[vost#0:0/h264_nvenc @ 1] Error while opening encoder - maybe incorrect parameters such as bit_rate, rate, width or height.\r\n";
    const AMF_ERR: &str = "[AMF @ 0000016a3681ac80] DLL amfrt64.dll failed to open\r\n[h264_amf @ 1] Failed to create  hardware device context (AMF) : Unknown error occurred\r\n";

    fn dev_machine() -> FakeFfmpeg {
        let mut f = FakeFfmpeg::new("gyan-full-9.0.1");
        for e in ["h264_nvenc", "hevc_nvenc", "av1_nvenc"] {
            f.encode_fail.insert(e, NVENC_ERR);
        }
        for e in ["h264_amf", "hevc_amf", "av1_amf"] {
            f.encode_fail.insert(e, AMF_ERR);
        }
        f.device_fail.insert("cuda", "[CUDA @ 1] Cannot load nvcuda.dll\r\nDevice creation failed: -1.\r\n");
        f.device_fail.insert("amf", "[AMF @ 1] DLL amfrt64.dll failed to open\r\n");
        f
    }

    fn run_probe(runner: &FakeFfmpeg, env: &FakeEnv, platform: Platform, app_dir: &Path, force: bool) -> Capabilities {
        run_probe_in(runner, env, platform, app_dir, force, Lang::ZhCn)
    }

    fn run_probe_in(
        runner: &FakeFfmpeg,
        env: &FakeEnv,
        platform: Platform,
        app_dir: &Path,
        force: bool,
        lang: Lang,
    ) -> Capabilities {
        let ctx = ProbeContext { env, runner, platform, app_dir: app_dir.to_path_buf(), user_path: None, lang };
        probe(&ctx, force, &|_| {})
    }

    fn han(t: &str) -> bool {
        t.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    }

    /// 环境页上由 VidForge 生成的文字（ffmpeg 原文除外）
    fn our_texts(c: &Capabilities) -> Vec<String> {
        let mut t = vec![c.status_detail.clone(), c.version_number.clone(), c.build_source.clone()];
        t.extend(c.notes.iter().cloned());
        t.extend(c.build_flags.iter().map(|f| f.affects.clone()));
        t.extend(c.tonemap.iter().map(|x| x.note.clone()));
        t.extend(c.external.iter().map(|x| x.purpose.clone()));
        t.extend(
            c.encoders.iter().filter(|e| e.failure == Some(FailureKind::NotBuilt)).filter_map(|e| e.error.clone()),
        );
        t
    }

    #[test]
    fn dev_machine_baseline_with_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let caps = run_probe(&dev_machine(), &installed_env(), Platform::Windows, dir.path(), true);
        assert_eq!(caps.status, EnvStatus::Ready);
        assert_eq!(caps.version_number, "9.0.1");
        assert_eq!(caps.build_source, "gyan.dev full");
        assert_eq!(caps.locate_source, Some(crate::model::LocateSource::Path));

        let e = |id| caps.encoder(id).unwrap();
        assert!(e(EncoderId::HevcQsv).usable && e(EncoderId::HevcQsv).ten_bit);
        assert!(e(EncoderId::Av1Qsv).usable && e(EncoderId::Av1Qsv).ten_bit);
        assert!(e(EncoderId::H264Qsv).usable);
        assert!(!e(EncoderId::H264Qsv).ten_bit, "h264_qsv 的像素格式列表里没有 p010le，不能判为支持 10bit");
        for id in [
            EncoderId::H264Nvenc,
            EncoderId::HevcNvenc,
            EncoderId::Av1Nvenc,
            EncoderId::H264Amf,
            EncoderId::HevcAmf,
            EncoderId::Av1Amf,
        ] {
            assert!(!e(id).usable);
            assert_eq!(e(id).failure, Some(FailureKind::DeviceMissing), "{id:?}");
        }
        assert!(e(EncoderId::HevcNvenc).error.as_deref().unwrap().contains("Cannot load nvcuda.dll"));
        assert!(e(EncoderId::HevcAmf).error.as_deref().unwrap().contains("amfrt64.dll"));

        assert!(caps.dolby_vision_encode);
        assert!(caps.dovi_split);
        assert_eq!(caps.pick_tonemap(), Some(ToneMapPipeline::Libplacebo));
        assert!(!caps.tonemap_available(ToneMapPipeline::ScaleVt));
        assert!(!caps.device_available("cuda"));
        assert!(caps.device_available("qsv"));
        assert!(caps.build_flags.iter().all(|f| f.present || f.name == "libfdk_aac"));
    }

    #[test]
    fn essentials_build_is_limited() {
        let dir = tempfile::tempdir().unwrap();
        let caps =
            run_probe(&FakeFfmpeg::new("gyan-essentials-9.0.1"), &installed_env(), Platform::Windows, dir.path(), true);
        assert_eq!(caps.status, EnvStatus::Ready);
        let av1 = caps.encoder(EncoderId::Libsvtav1).unwrap();
        assert!(!av1.usable);
        assert_eq!(av1.failure, Some(FailureKind::NotBuilt));
        assert!(!caps.tonemap_available(ToneMapPipeline::Libplacebo));
        assert!(!caps.tonemap_available(ToneMapPipeline::TonemapOpencl));
        assert!(caps.tonemap_available(ToneMapPipeline::Zscale), "essentials 有 libzimg");
        let missing: Vec<&str> = caps.build_flags.iter().filter(|f| !f.present).map(|f| f.name.as_str()).collect();
        for f in ["libsvtav1", "libplacebo", "opencl"] {
            assert!(missing.contains(&f), "应标出缺少 {f}");
        }
    }

    #[test]
    fn old_build_is_too_old() {
        let dir = tempfile::tempdir().unwrap();
        let caps =
            run_probe(&FakeFfmpeg::new("gyan-essentials-6.0"), &installed_env(), Platform::Windows, dir.path(), true);
        assert_eq!(caps.status, EnvStatus::TooOld);
        assert!(caps.status_detail.contains("7.1"));
        assert!(!caps.dolby_vision_encode);
        assert!(!caps.can_transcode());
    }

    #[test]
    fn missing_ffmpeg() {
        let dir = tempfile::tempdir().unwrap();
        let caps = run_probe(&FakeFfmpeg::new("none"), &FakeEnv(HashSet::new()), Platform::Windows, dir.path(), true);
        assert_eq!(caps.status, EnvStatus::Missing);
        assert!(caps.searched.iter().any(|s| s == r"C:\ff\bin"));
        assert!(caps.encoders.iter().all(|e| !e.usable), "找不到 ffmpeg 时任何编码器都不能标为可用");
    }

    #[test]
    fn present_but_unusable_ffmpeg_is_broken_not_missing() {
        let dir = tempfile::tempdir().unwrap();
        // 目录里有 ffmpeg.exe 却没有 ffprobe.exe（常见于只拷了一个文件）
        let env = FakeEnv([PathBuf::from(r"C:\ff\bin").join("ffmpeg.exe")].into_iter().collect());
        let caps = run_probe(&FakeFfmpeg::new("gyan-full-9.0.1"), &env, Platform::Windows, dir.path(), true);
        assert_eq!(caps.status, EnvStatus::Broken);
        assert!(caps.status_detail.contains("没有 ffprobe"), "{}", caps.status_detail);
        assert!(!caps.can_transcode());
    }

    #[test]
    fn cache_hit_skips_probing_and_force_refreshes() {
        let dir = tempfile::tempdir().unwrap();
        let env = installed_env();
        let first = dev_machine();
        run_probe(&first, &env, Platform::Windows, dir.path(), false);
        assert!(first.calls.lock().unwrap().len() > 10);

        let second = dev_machine();
        let cached = run_probe(&second, &env, Platform::Windows, dir.path(), false);
        assert_eq!(cached.status, EnvStatus::Ready);
        let calls = second.calls.lock().unwrap();
        assert!(calls.iter().all(|a| a.iter().any(|s| s == "-version")), "缓存命中时只允许跑 -version：{calls:?}");
        drop(calls);

        let third = dev_machine();
        run_probe(&third, &env, Platform::Windows, dir.path(), true);
        assert!(third.calls.lock().unwrap().len() > 10, "force 时必须重新探测");
    }

    #[test]
    fn cache_hit_still_reports_how_ffmpeg_was_found_this_time() {
        let dir = tempfile::tempdir().unwrap();
        let env = installed_env();
        let first = run_probe(&dev_machine(), &env, Platform::Windows, dir.path(), false);
        assert_eq!(first.locate_source, Some(crate::model::LocateSource::Path));

        // 用户在设置里指定了同一个目录：指纹不变、命中缓存，但来源要如实变成"设置中指定"
        let runner = dev_machine();
        let ctx = ProbeContext {
            env: &env,
            runner: &runner,
            platform: Platform::Windows,
            app_dir: dir.path().to_path_buf(),
            user_path: Some(PathBuf::from(r"C:\ff\bin")),
            lang: Lang::ZhCn,
        };
        let second = probe(&ctx, false, &|_| {});
        assert_eq!(second.locate_source, Some(crate::model::LocateSource::User));
        assert!(runner.calls.lock().unwrap().iter().all(|a| a.iter().any(|s| s == "-version")), "应命中缓存");
    }

    #[test]
    fn stale_schema_cache_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let mut caps = Capabilities::placeholder(EnvStatus::Ready, "");
        caps.fingerprint = "x".into();
        fs::write(dir.path().join(CACHE_FILE), serde_json::to_string(&CacheFile { schema: 0, caps }).unwrap()).unwrap();
        assert!(load_cache(dir.path()).is_none());
    }

    #[test]
    fn ten_bit_is_downgraded_when_test_fails() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = dev_machine();
        f.ten_fail.insert("hevc_qsv");
        let caps = run_probe(&f, &installed_env(), Platform::Windows, dir.path(), true);
        let q = caps.encoder(EncoderId::HevcQsv).unwrap();
        assert!(q.usable && !q.ten_bit);
    }

    #[test]
    fn vulkan_failure_disables_libplacebo_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = dev_machine();
        f.device_fail.insert("vulkan", "[Vulkan @ 1] No devices found\r\nDevice creation failed: -22.\r\n");
        let caps = run_probe(&f, &installed_env(), Platform::Windows, dir.path(), true);
        let lp = caps.tonemap.iter().find(|t| t.id == ToneMapPipeline::Libplacebo).unwrap();
        assert!(!lp.available);
        assert!(lp.note.contains("Vulkan"));
        assert_eq!(caps.pick_tonemap(), Some(ToneMapPipeline::TonemapOpencl));
    }

    #[test]
    fn english_texts_come_from_the_cache_without_reprobing() {
        let dir = tempfile::tempdir().unwrap();
        let env = installed_env();
        // 先用中文探测并写缓存，再用英文命中缓存：说明换成英文，而且没有重新探测
        let zh = run_probe_in(
            &FakeFfmpeg::new("gyan-essentials-9.0.1"),
            &env,
            Platform::Windows,
            dir.path(),
            true,
            Lang::ZhCn,
        );
        assert!(zh.build_flags.iter().any(|f| f.affects == "AV1 软编（含 HDR10）"));
        let runner = FakeFfmpeg::new("gyan-essentials-9.0.1");
        let en = run_probe_in(&runner, &env, Platform::Windows, dir.path(), false, Lang::En);
        assert!(runner.calls.lock().unwrap().iter().all(|a| a.iter().any(|s| s == "-version")), "应命中缓存");
        let texts = our_texts(&en);
        assert!(texts.iter().all(|t| !han(t)), "英文界面仍有中文：{texts:?}");
        assert!(en.build_flags.iter().any(|f| f.affects == "AV1 software encoding (with HDR10)"));
        let lp = en.tonemap.iter().find(|t| t.id == ToneMapPipeline::Libplacebo).unwrap();
        assert_eq!(lp.note, "This ffmpeg was built without libplacebo");
        let av1 = en.encoder(EncoderId::Libsvtav1).unwrap();
        assert_eq!(av1.error.as_deref(), Some("This ffmpeg was built without this encoder"));
        // 缓存本身仍是中文原文，再用中文读回来一字不差
        let back = run_probe_in(
            &FakeFfmpeg::new("gyan-essentials-9.0.1"),
            &env,
            Platform::Windows,
            dir.path(),
            false,
            Lang::ZhCn,
        );
        assert_eq!(our_texts(&back), our_texts(&zh));
    }

    #[test]
    fn english_status_texts_for_problem_environments() {
        let dir = tempfile::tempdir().unwrap();
        let old = run_probe_in(
            &FakeFfmpeg::new("gyan-essentials-6.0"),
            &installed_env(),
            Platform::Windows,
            dir.path(),
            true,
            Lang::En,
        );
        assert!(old.status_detail.starts_with("ffmpeg 6.0 is older than the required 7.1"), "{}", old.status_detail);
        let none = run_probe_in(
            &FakeFfmpeg::new("none"),
            &FakeEnv(HashSet::new()),
            Platform::Windows,
            dir.path(),
            true,
            Lang::En,
        );
        assert!(none.status_detail.starts_with("ffmpeg and ffprobe were not found"), "{}", none.status_detail);
        let env = FakeEnv([PathBuf::from(r"C:\ff\bin").join("ffmpeg.exe")].into_iter().collect());
        let broken =
            run_probe_in(&FakeFfmpeg::new("gyan-full-9.0.1"), &env, Platform::Windows, dir.path(), true, Lang::En);
        assert!(broken.status_detail.contains("has ffmpeg but no ffprobe"), "{}", broken.status_detail);
        for c in [&old, &none, &broken] {
            assert!(our_texts(c).iter().all(|t| !han(t)), "{:?}", our_texts(c));
        }
    }

    #[test]
    fn device_failures_keep_the_raw_error_in_both_languages() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = dev_machine();
        f.device_fail.insert("vulkan", "[Vulkan @ 1] No devices found\r\nDevice creation failed: -22.\r\n");
        let en = run_probe_in(&f, &installed_env(), Platform::Windows, dir.path(), true, Lang::En);
        let lp = en.tonemap.iter().find(|t| t.id == ToneMapPipeline::Libplacebo).unwrap();
        assert!(lp.note.starts_with("The Vulkan device failed to initialize: "), "{}", lp.note);
        assert!(matches!(&lp.block, Some(TonemapBlock::Device { device, .. }) if device == "Vulkan"));
    }

    #[test]
    fn test_encode_uses_null_muxer_and_three_frames() {
        for enc in EncoderId::ALL {
            let a = test_encode_args(enc, false).unwrap();
            let joined = a.join(" ");
            assert!(joined.ends_with("-f null -"), "{enc:?}: 必须用 -f null -，不能用 NUL");
            assert!(joined.contains("-frames:v 3"), "{enc:?}: 硬编有 lookahead，至少 3 帧");
        }
        assert!(test_encode_args(EncoderId::H264Videotoolbox, true).is_none(), "VideoToolbox 的 H.264 没有 10bit");
        let q = test_encode_args(EncoderId::HevcQsv, true).unwrap().join(" ");
        assert!(q.contains("-pix_fmt p010le -profile:v main10"));
    }

    #[test]
    fn platform_encoder_sets() {
        let win = platform_encoders(Platform::Windows);
        assert!(win.contains(&EncoderId::HevcQsv) && win.contains(&EncoderId::HevcAmf));
        assert!(!win.contains(&EncoderId::HevcVideotoolbox));
        let mac = platform_encoders(Platform::Macos);
        assert_eq!(
            mac,
            vec![
                EncoderId::Libx264,
                EncoderId::Libx265,
                EncoderId::Libsvtav1,
                EncoderId::H264Videotoolbox,
                EncoderId::HevcVideotoolbox
            ]
        );
    }

    // 保证 SystemEnv 可以构造（真实环境的探测在 tests/probe_real.rs）
    #[test]
    fn system_env_constructs() {
        let _ = SystemEnv.var("PATH");
    }
}
