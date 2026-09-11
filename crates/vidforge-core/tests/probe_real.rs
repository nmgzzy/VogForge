//! 在真实 ffmpeg 上跑完整的三层探测。
//!
//! 环境不满足时跳过并打印原因，不假定机器上有 GPU 或特定构建：
//! - `VIDFORGE_TEST_FFMPEG`：主构建目录，默认开发机的 `C:\Program1\ffmpeg\bin`
//! - `VIDFORGE_TEST_FFMPEG_ESSENTIALS`：能力受限的 gyan essentials 构建
//! - `VIDFORGE_TEST_FFMPEG_OLD`：低于 7.1 的旧构建
//!
//! 只有 Intel 显卡的机器（开发机：Core Ultra 的 Arc 核显、无 NVIDIA/AMD 卡）上额外核对
//! docs/ffmpeg-facts.md 7.2 节的实测基线；`VIDFORGE_SKIP_BASELINE` 可关闭这项核对。

use std::path::PathBuf;
use std::time::Instant;

use vidforge_core::ffmpeg::capability::{ProbeContext, probe};
use vidforge_core::ffmpeg::exec::SystemRunner;
use vidforge_core::ffmpeg::locate::SystemEnv;
use vidforge_core::model::{Capabilities, EncoderId, EnvStatus, FailureKind, LocateSource, Platform, ToneMapPipeline};

fn build_dir(var: &str, default: Option<&str>) -> Option<PathBuf> {
    let dir = std::env::var_os(var).map(PathBuf::from).or_else(|| default.map(PathBuf::from))?;
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    dir.join(exe).is_file().then_some(dir)
}

fn run(dir: PathBuf) -> Capabilities {
    let app = tempfile::tempdir().unwrap();
    let ctx = ProbeContext {
        env: &SystemEnv,
        runner: &SystemRunner,
        platform: Platform::current(),
        app_dir: app.path().to_path_buf(),
        user_path: Some(dir),
    };
    let t = Instant::now();
    let caps = probe(&ctx, true, &|_| {});
    eprintln!("探测耗时 {:?}：{} {}", t.elapsed(), caps.version, caps.build_source);
    caps
}

#[test]
fn main_build() {
    let Some(dir) = build_dir("VIDFORGE_TEST_FFMPEG", cfg!(windows).then_some(r"C:\Program1\ffmpeg\bin")) else {
        eprintln!("跳过：没有可用的 ffmpeg（设置 VIDFORGE_TEST_FFMPEG 指定目录）");
        return;
    };
    let caps = run(dir);
    assert_eq!(caps.status, EnvStatus::Ready, "{}", caps.status_detail);
    assert_eq!(caps.locate_source, Some(LocateSource::User));
    assert!(caps.encoder_usable(EncoderId::Libx265), "libx265 应可用");
    assert!(caps.encoder_usable(EncoderId::Libx264), "libx264 应可用");

    // 不可用的硬件编码器必须带有分类与原因
    for e in caps.encoders.iter().filter(|e| !e.usable) {
        assert!(e.failure.is_some() && e.error.as_deref().is_some_and(|s| !s.is_empty()), "{:?} 缺少失败原因", e.id);
    }

    // 开发机的注册表显卡名是 "Intel(R) Graphics"（Core Ultra 7 356H 的 Arc 核显）。
    // 其他只有 Intel 核显的机器若型号较老（不支持 AV1），设置 VIDFORGE_SKIP_BASELINE 跳过
    let intel_only = !caps.gpus.is_empty()
        && caps.gpus.iter().all(|g| g.name.contains("Intel"))
        && std::env::var_os("VIDFORGE_SKIP_BASELINE").is_none();
    if !intel_only {
        eprintln!("非开发机，跳过基线核对。GPU：{:?}", caps.gpus);
        return;
    }
    // docs/ffmpeg-facts.md 7.2 节：QSV 六项 PASS（vp9 不在 VidForge 的编码器列表里），NVENC / AMF 设备缺失
    let q = |id| caps.encoder(id).unwrap();
    assert!(q(EncoderId::H264Qsv).usable);
    assert!(q(EncoderId::HevcQsv).usable && q(EncoderId::HevcQsv).ten_bit);
    assert!(q(EncoderId::Av1Qsv).usable && q(EncoderId::Av1Qsv).ten_bit);
    assert!(!q(EncoderId::H264Qsv).ten_bit, "h264_qsv 不支持 10bit");
    for id in [EncoderId::H264Nvenc, EncoderId::HevcNvenc, EncoderId::Av1Nvenc] {
        assert_eq!(q(id).failure, Some(FailureKind::DeviceMissing));
        assert!(q(id).error.as_deref().unwrap().contains("Cannot load nvcuda.dll"), "{:?}", q(id).error);
    }
    for id in [EncoderId::H264Amf, EncoderId::HevcAmf, EncoderId::Av1Amf] {
        assert_eq!(q(id).failure, Some(FailureKind::DeviceMissing));
        assert!(q(id).error.as_deref().unwrap().contains("amfrt64.dll"), "{:?}", q(id).error);
    }
    assert!(caps.dolby_vision_encode);
    for p in [ToneMapPipeline::Libplacebo, ToneMapPipeline::TonemapOpencl, ToneMapPipeline::Zscale] {
        assert!(caps.tonemap_available(p), "{p:?} 应可用");
    }
    assert!(caps.device_available("qsv") && !caps.device_available("cuda"));
}

#[test]
fn essentials_build() {
    let Some(dir) = build_dir("VIDFORGE_TEST_FFMPEG_ESSENTIALS", None) else {
        eprintln!("跳过：未设置 VIDFORGE_TEST_FFMPEG_ESSENTIALS");
        return;
    };
    let caps = run(dir);
    assert_eq!(caps.status, EnvStatus::Ready);
    assert_eq!(caps.build_source, "gyan.dev essentials");
    assert_eq!(caps.encoder(EncoderId::Libsvtav1).unwrap().failure, Some(FailureKind::NotBuilt));
    assert!(!caps.tonemap_available(ToneMapPipeline::Libplacebo));
    assert!(!caps.tonemap_available(ToneMapPipeline::TonemapOpencl));
    assert_eq!(caps.pick_tonemap(), Some(ToneMapPipeline::Zscale));
}

#[test]
fn old_build() {
    let Some(dir) = build_dir("VIDFORGE_TEST_FFMPEG_OLD", None) else {
        eprintln!("跳过：未设置 VIDFORGE_TEST_FFMPEG_OLD");
        return;
    };
    let caps = run(dir);
    assert_eq!(caps.status, EnvStatus::TooOld);
    assert!(!caps.dolby_vision_encode);
    assert!(!caps.can_transcode());
}
