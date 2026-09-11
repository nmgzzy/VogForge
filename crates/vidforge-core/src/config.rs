//! 应用目录与用户设置。设置写在 `~/.vidforge/config.json`。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 应用数据目录：`VIDFORGE_HOME` 环境变量优先（测试用），否则 `~/.vidforge`
pub fn app_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("VIDFORGE_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    #[allow(deprecated)]
    let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".vidforge")
}

/// 应用下载的 ffmpeg 构建存放处（定位顺序中的"应用目录"）
pub fn bundled_ffmpeg_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("ffmpeg").join("bin")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ConflictPolicy {
    Skip,
    #[default]
    Rename,
    Overwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AfterAction {
    /// 什么都不做（默认）
    #[default]
    None,
    /// 打开输出目录
    Open,
    /// 把源文件移到回收站（每次确认）
    Trash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum Lang {
    #[default]
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en")]
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, optional_fields)]
pub struct Settings {
    /// 用户指定的 ffmpeg 目录或可执行文件；None 表示自动查找
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ffmpeg_path: Option<String>,
    /// 输出目录；None 表示源文件旁的 VidForge 文件夹
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<String>,
    pub naming_template: String,
    pub keep_tree: bool,
    pub conflict: ConflictPolicy,
    pub after: AfterAction,
    pub hw_encode: bool,
    pub hw_decode: bool,
    pub cpu_slots: u32,
    pub gpu_slots: u32,
    pub notify: bool,
    pub language: Lang,
    pub theme: ThemePref,
    /// 首次启动引导已经走完（需求 F-9.5）
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            ffmpeg_path: None,
            output_dir: None,
            naming_template: "{name}_{height}p_{codec}".to_string(),
            keep_tree: true,
            conflict: ConflictPolicy::Rename,
            after: AfterAction::None,
            hw_encode: true,
            hw_decode: true,
            cpu_slots: 1,
            gpu_slots: 1,
            notify: true,
            language: Lang::ZhCn,
            theme: ThemePref::System,
            onboarded: false,
        }
    }
}

impl Settings {
    /// 把越界的数值拉回合理范围（手改配置文件时可能写坏）
    pub fn sanitized(mut self) -> Settings {
        self.cpu_slots = self.cpu_slots.clamp(1, 4);
        self.gpu_slots = self.gpu_slots.clamp(1, 2);
        if self.naming_template.trim().is_empty() {
            self.naming_template = Settings::default().naming_template;
        }
        self.ffmpeg_path = self.ffmpeg_path.filter(|p| !p.trim().is_empty());
        self.output_dir = self.output_dir.filter(|p| !p.trim().is_empty());
        self
    }
}

fn settings_path(app_dir: &Path) -> PathBuf {
    app_dir.join("config.json")
}

/// 读取设置；文件不存在或损坏时返回默认值（损坏的文件会被保留为 .bak 以便排查）
pub fn load_settings(app_dir: &Path) -> Settings {
    let path = settings_path(app_dir);
    let Ok(text) = fs::read_to_string(&path) else { return Settings::default() };
    match serde_json::from_str::<Settings>(&text) {
        Ok(s) => s.sanitized(),
        Err(_) => {
            let _ = fs::rename(&path, path.with_extension("json.bak"));
            Settings::default()
        }
    }
}

pub fn save_settings(app_dir: &Path, settings: &Settings) -> io::Result<()> {
    write_json_atomic(&settings_path(app_dir), settings)
}

/// 先写临时文件再改名，避免写到一半崩溃留下损坏的 JSON
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_settings(dir.path()), Settings::default());
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings {
            ffmpeg_path: Some(r"C:\ff\bin".into()),
            gpu_slots: 2,
            language: Lang::En,
            ..Settings::default()
        };
        save_settings(dir.path(), &s).unwrap();
        assert_eq!(load_settings(dir.path()), s);
    }

    #[test]
    fn saving_again_replaces_the_existing_file() {
        // std::fs::rename 在 Windows 上会替换已存在的目标文件（MoveFileEx + REPLACE_EXISTING），
        // 这里把"第二次保存必须生效"钉住，防止换实现时退化
        let dir = tempfile::tempdir().unwrap();
        save_settings(dir.path(), &Settings { gpu_slots: 2, ..Settings::default() }).unwrap();
        save_settings(dir.path(), &Settings { gpu_slots: 1, notify: false, ..Settings::default() }).unwrap();
        let s = load_settings(dir.path());
        assert_eq!((s.gpu_slots, s.notify), (1, false));
        assert!(!dir.path().join("config.json.tmp").exists(), "不应残留临时文件");
    }

    #[test]
    fn partial_file_fills_defaults_and_clamps() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.json"), r#"{"cpuSlots": 99, "ffmpegPath": "  "}"#).unwrap();
        let s = load_settings(dir.path());
        assert_eq!(s.cpu_slots, 4);
        assert_eq!(s.ffmpeg_path, None);
        assert!(s.hw_encode);
    }

    #[test]
    fn corrupt_file_is_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.json"), "{not json").unwrap();
        assert_eq!(load_settings(dir.path()), Settings::default());
        assert!(dir.path().join("config.json.bak").exists());
    }

    #[test]
    fn serialized_names_are_camel_case() {
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert!(json.contains("\"namingTemplate\""));
        assert!(json.contains("\"language\":\"zh-CN\""));
        assert!(!json.contains("ffmpegPath"), "None 字段应省略");
    }
}
