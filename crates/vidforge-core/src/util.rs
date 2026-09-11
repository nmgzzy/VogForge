//! 零散的通用工具。

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 当前 UTC 时间的 ISO 8601 字符串，如 `2026-09-11T06:32:08Z`
pub fn now_iso() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    iso_from_unix(secs)
}

pub fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Unix 秒数转 ISO 8601（UTC）。算法来自 Howard Hinnant 的 civil_from_days。
pub fn iso_from_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// 用固定数量的线程并行处理列表，结果保持输入顺序。每完成一项调用一次 `on_done(已完成数)`。
pub fn par_map<T, R, F, P>(items: &[T], workers: usize, f: F, on_done: P) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
    P: Fn(usize) + Sync,
{
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    let workers = workers.clamp(1, items.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= items.len() {
                        break;
                    }
                    let r = f(&items[i]);
                    slots.lock().unwrap()[i] = Some(r);
                    on_done(done.fetch_add(1, Ordering::SeqCst) + 1);
                }
            });
        }
    });
    slots.into_inner().unwrap().into_iter().map(|r| r.expect("每一项都已处理")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates() {
        assert_eq!(iso_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_unix(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso_from_unix(1_789_108_328), "2026-09-11T06:32:08Z");
    }

    #[test]
    fn par_map_keeps_order_and_reports_progress() {
        let items: Vec<u32> = (0..20).collect();
        let max = AtomicUsize::new(0);
        let out = par_map(
            &items,
            4,
            |x| x * 2,
            |n| {
                max.fetch_max(n, Ordering::SeqCst);
            },
        );
        assert_eq!(out, items.iter().map(|x| x * 2).collect::<Vec<_>>());
        assert_eq!(max.load(Ordering::SeqCst), 20);
        assert!(par_map(&Vec::<u32>::new(), 4, |x| *x, |_| {}).is_empty());
    }
}
