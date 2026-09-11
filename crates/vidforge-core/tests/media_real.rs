//! 用真实 ffmpeg 合成素材，再走完整的导入流程（ffprobe 分析）核对结果。
//!
//! 这也是 tests/fixtures/probe 下"真实输出"那批 fixture 的生成方法。找不到 ffmpeg 时跳过。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use vidforge_core::ffmpeg::exec::{Runner, SystemRunner, args};
use vidforge_core::import::import_paths;
use vidforge_core::model::{HdrKind, MasteringPrimaries, MediaInfo, SourceHint};

fn ffmpeg_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("VIDFORGE_TEST_FFMPEG")
        .map(PathBuf::from)
        .or_else(|| cfg!(windows).then(|| PathBuf::from(r"C:\Program1\ffmpeg\bin")))?;
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    dir.join(exe).is_file().then_some(dir)
}

fn exe(dir: &Path, name: &str) -> PathBuf {
    dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() })
}

fn ffmpeg(bin: &Path, a: &[&str]) {
    let mut full = args(["-hide_banner", "-nostdin", "-v", "error", "-y"]);
    full.extend(a.iter().map(|s| s.to_string()));
    let out = SystemRunner.run(&exe(bin, "ffmpeg"), &full, Duration::from_secs(120)).unwrap();
    assert!(out.success(), "ffmpeg {a:?} 失败：{}", out.stderr);
}

const X265_HDR10: &str = "log-level=error:hdr10=1:repeat-headers=1:colorprim=bt2020:transfer=smpte2084:\
colormatrix=bt2020nc:master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1):max-cll=1000,400";

/// 生成与 fixture 同款的合成素材
fn synthesize(bin: &Path, d: &Path) {
    let p = |n: &str| d.join(n).to_string_lossy().to_string();
    let src_hd = "testsrc2=s=1280x720:r=24";
    let hevc = p("hdr10-hevc.mkv");
    ffmpeg(
        bin,
        &[
            "-f",
            "lavfi",
            "-i",
            src_hd,
            "-t",
            "2",
            "-pix_fmt",
            "yuv420p10le",
            "-c:v",
            "libx265",
            "-x265-params",
            X265_HDR10,
            &hevc,
        ],
    );
    ffmpeg(
        bin,
        &[
            "-i",
            &hevc,
            "-c:v",
            "libsvtav1",
            "-svtav1-params",
            "enable-hdr=1",
            "-pix_fmt",
            "yuv420p10le",
            &p("hdr10-av1.mkv"),
        ],
    );

    // 手机 HLG：色彩标签要用 setparams 写进帧里（ffmpeg 9 的 -color_trc 输出选项不生效，见技术事实文档 12 节），
    // 旋转在流复制时用 -display_rotation 写入，避免被自动旋转掉
    let tmp = p("tmp-hlg.mov");
    ffmpeg(
        bin,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=1920x1080:r=30",
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:sample_rate=48000",
            "-t",
            "2",
            "-vf",
            "format=yuv420p10le,setparams=color_primaries=bt2020:color_trc=arib-std-b67:colorspace=bt2020nc:range=tv",
            "-c:v",
            "libx265",
            "-x265-params",
            "log-level=error",
            "-tag:v",
            "hvc1",
            "-c:a",
            "aac",
            "-ac",
            "2",
            &tmp,
        ],
    );
    ffmpeg(
        bin,
        &[
            "-display_rotation:v:0",
            "-90",
            "-i",
            &tmp,
            "-c",
            "copy",
            "-metadata",
            "com.apple.quicktime.make=Apple",
            "-metadata",
            "com.apple.quicktime.model=iPhone 15 Pro",
            "-movflags",
            "+use_metadata_tags",
            &p("IMG_0001.MOV"),
        ],
    );
    fs::remove_file(&tmp).unwrap();

    // 可变帧率：前 2 秒 30fps，之后每 3 帧留 1 帧，保留原时间戳
    for ext in ["mp4", "mkv"] {
        ffmpeg(
            bin,
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=1280x720:r=30",
                "-f",
                "lavfi",
                "-i",
                "sine=f=300:sample_rate=48000",
                "-t",
                "6",
                "-vf",
                "select='lt(n,60)+not(mod(n,3))'",
                "-fps_mode",
                "vfr",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-c:a",
                "aac",
                "-ac",
                "2",
                &p(&format!("vfr.{ext}")),
            ],
        );
    }
    ffmpeg(
        bin,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=1280x720:r=24000/1001",
            "-t",
            "5",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            &p("cfr2398.mkv"),
        ],
    );

    // 类蓝光 remux：TrueHD + E-AC-3 + FLAC、文本字幕、两个章节、一个附件
    fs::write(d.join("sub.srt"), "1\n00:00:00,000 --> 00:00:01,500\nHello\n").unwrap();
    fs::write(
        d.join("chapters.txt"),
        ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1500\ntitle=A\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=1500\nEND=3000\ntitle=B\n",
    )
    .unwrap();
    ffmpeg(
        bin,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=640x360:r=24000/1001",
            "-f",
            "lavfi",
            "-i",
            "sine=f=220:sample_rate=48000",
            "-i",
            &p("sub.srt"),
            "-i",
            &p("chapters.txt"),
            "-t",
            "3",
            "-filter_complex",
            "[1:a]pan=5.1|c0=c0|c1=c0|c2=c0|c3=c0|c4=c0|c5=c0,asplit=2[a][b]",
            "-map",
            "0:v",
            "-map",
            "[a]",
            "-map",
            "[b]",
            "-map",
            "1:a",
            "-map",
            "2:s",
            "-map_chapters",
            "3",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a:0",
            "truehd",
            "-strict",
            "-2",
            "-c:a:1",
            "eac3",
            "-c:a:2",
            "flac",
            "-c:s",
            "srt",
            "-metadata:s:a:2",
            "language=jpn",
            "-attach",
            &p("sub.srt"),
            "-metadata:s:t",
            "mimetype=application/x-subrip",
            &p("remux.mkv"),
        ],
    );
    fs::remove_file(d.join("sub.srt")).unwrap();
    fs::remove_file(d.join("chapters.txt")).unwrap();
}

fn by_name<'a>(media: &'a [MediaInfo], name: &str) -> &'a MediaInfo {
    media.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("缺少 {name}"))
}

#[test]
fn synthesized_media_through_the_real_import_path() {
    let Some(bin) = ffmpeg_dir() else {
        eprintln!("跳过：没有可用的 ffmpeg（设置 VIDFORGE_TEST_FFMPEG 指定目录）");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    synthesize(&bin, dir.path());
    fs::write(dir.path().join("readme.txt"), "not a video").unwrap();
    fs::write(dir.path().join("broken.mp4"), b"\x00\x00\x00\x18ftypmp42 truncated").unwrap();

    let r = import_paths(&[dir.path().to_path_buf()], &exe(&bin, "ffprobe"), &SystemRunner, 4, &|_| {});
    assert_eq!(r.skipped, 1, "readme.txt 应被跳过");
    assert_eq!(r.failures.len(), 1, "broken.mp4 应失败：{:?}", r.failures);
    assert!(r.failures[0].reason.contains("文件不完整") || r.failures[0].reason.contains("无法"), "{:?}", r.failures);
    assert_eq!(r.media.len(), 7);

    for name in ["hdr10-hevc.mkv", "hdr10-av1.mkv"] {
        let v = &by_name(&r.media, name).video[0];
        let md = v.hdr10.as_ref().unwrap_or_else(|| panic!("{name} 应有 HDR10 元数据"));
        assert!((md.max_luminance - 1000.0).abs() < 1e-6, "{name}: {}", md.max_luminance);
        assert_eq!(md.max_cll, Some(1000.0));
        assert_eq!(md.mastering_primaries, MasteringPrimaries::P3);
    }
    for name in ["hdr10-hevc.mkv", "hdr10-av1.mkv"] {
        assert_eq!(by_name(&r.media, name).video[0].color.hdr_kind, HdrKind::Hdr10, "{name}");
    }

    let hlg = by_name(&r.media, "IMG_0001.MOV");
    assert_eq!(hlg.video[0].color.hdr_kind, HdrKind::Hlg);
    assert_eq!(hlg.video[0].rotation, -90);
    assert_eq!(hlg.source_hint, SourceHint::Iphone);
    assert_eq!(hlg.device.as_deref(), Some("Apple iPhone 15 Pro"));

    assert!(by_name(&r.media, "vfr.mp4").video[0].is_vfr, "MP4 靠帧率字段判出");
    assert!(by_name(&r.media, "vfr.mkv").video[0].is_vfr, "MKV 靠包时间戳判出");
    assert!(!by_name(&r.media, "cfr2398.mkv").video[0].is_vfr);

    let remux = by_name(&r.media, "remux.mkv");
    assert_eq!(remux.audio.len(), 3);
    assert!(remux.audio[0].lossless && remux.audio[2].lossless && !remux.audio[1].lossless);
    assert_eq!((remux.chapters, remux.attachments, remux.subtitle.len()), (2, 1, 1));
    assert!(r.media.iter().all(|m| m.import_root.as_deref() == Some(dir.path().to_string_lossy().as_ref())));
}
