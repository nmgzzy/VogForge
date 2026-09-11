# ffprobe fixture

每个目录对应一个素材，三个文件对应 `ffmpeg::probe` 调用 ffprobe 的三次输出：

| 文件 | 命令 |
|---|---|
| `info.json` | `ffprobe -v error -show_format -show_streams -show_chapters -of json <文件>` |
| `frame.json` | `ffprobe -v error -select_streams v:0 -read_intervals "%+#1" -show_frames -show_entries frame=side_data_list,color_range,color_space,color_primaries,color_transfer -of json <文件>` |
| `packets.csv` | `ffprobe -v error -select_streams v:0 -read_intervals "%+#120" -show_packets -show_entries packet=pts_time -of csv=p=0 <文件>` |

来源：

- **真实输出**（开发机 ffmpeg 9.0.1 合成素材后由 ffprobe 生成，生成命令见 `tests/media_real.rs`）：
  `hdr10-hevc`、`hdr10-av1`、`hlg-iphone`、`vfr-mp4`、`vfr-mkv`、`cfr2398-mkv`、`remux-mkv`、`camera-mp4`
- **手工构造**（手头没有真实片源，也无法用 ffmpeg 合成）：
  - `iphone-dv84`：以 `hlg-iphone` 为底，加上杜比视界 8.4 的配置记录与逐帧 RPU side data
  - `bluray-p7`：以 `remux-mkv` 为底，改成杜比视界 P7 FEL、HDR10、TrueHD Atmos、DTS-HD MA、PGS 字幕

  手工构造部分的字段名（`DOVI configuration record`、`dv_bl_signal_compatibility_id`、`disable_residual_flag`、
  `Dolby TrueHD + Dolby Atmos`、`DTS-HD MA` 等）已在 ffprobe 9.0.1 可执行文件中逐一核对存在。
