//! 显卡与驱动信息。用于环境页展示与能力缓存的失效判断（驱动升级后要重测）。

use crate::ffmpeg::exec::Runner;
use crate::model::GpuInfo;

/// 读取本机显卡列表。Windows 读注册表的显示适配器类；macOS 用 system_profiler。
pub fn gpus(runner: &dyn Runner) -> Vec<GpuInfo> {
    #[cfg(windows)]
    {
        let _ = runner;
        windows_gpus()
    }
    #[cfg(target_os = "macos")]
    {
        macos_gpus(runner)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = runner;
        Vec::new()
    }
}

#[cfg(windows)]
fn windows_gpus() -> Vec<GpuInfo> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    // 显示适配器的设备类 GUID
    const CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let Ok(class) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(CLASS) else { return Vec::new() };
    let mut out = Vec::new();
    for name in class.enum_keys().flatten() {
        if name.len() != 4 || !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(key) = class.open_subkey(&name) else { continue };
        let Ok(desc) = key.get_value::<String, _>("DriverDesc") else { continue };
        let driver = key.get_value::<String, _>("DriverVersion").unwrap_or_default();
        if is_virtual_adapter(&desc) {
            continue;
        }
        out.push(GpuInfo { name: desc, driver });
    }
    out
}

#[cfg(target_os = "macos")]
fn macos_gpus(runner: &dyn Runner) -> Vec<GpuInfo> {
    use crate::ffmpeg::exec::args;
    use std::path::Path;
    use std::time::Duration;
    let os = runner
        .run(Path::new("/usr/bin/sw_vers"), &args(["-productVersion"]), Duration::from_secs(5))
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();
    let text = runner
        .run(Path::new("/usr/sbin/system_profiler"), &args(["SPDisplaysDataType"]), Duration::from_secs(15))
        .map(|o| o.stdout)
        .unwrap_or_default();
    parse_system_profiler(&text, &os)
}

/// 解析 `system_profiler SPDisplaysDataType` 的 `Chipset Model:` 行。macOS 的显卡驱动随系统更新，
/// 所以驱动版本记为系统版本。
pub fn parse_system_profiler(text: &str, os_version: &str) -> Vec<GpuInfo> {
    let driver = if os_version.is_empty() { String::new() } else { format!("macOS {os_version}") };
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("Chipset Model:"))
        .map(|name| GpuInfo { name: name.trim().to_string(), driver: driver.clone() })
        .collect()
}

#[cfg_attr(not(windows), allow(dead_code))]
fn is_virtual_adapter(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("microsoft basic")
        || n.contains("remote display")
        || n.contains("virtual display")
        || n.contains("parsec")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_profiler_output() {
        let text = "Graphics/Displays:\n\n    Apple M2 Pro:\n\n      Chipset Model: Apple M2 Pro\n      Type: GPU\n      Bus: Built-In\n";
        let g = parse_system_profiler(text, "14.5");
        assert_eq!(g, vec![GpuInfo { name: "Apple M2 Pro".into(), driver: "macOS 14.5".into() }]);
    }

    #[test]
    fn virtual_adapters_are_skipped() {
        assert!(is_virtual_adapter("Microsoft Basic Display Adapter"));
        assert!(is_virtual_adapter("Microsoft Remote Display Adapter"));
        assert!(!is_virtual_adapter("Intel(R) Arc(TM) Graphics"));
    }
}
