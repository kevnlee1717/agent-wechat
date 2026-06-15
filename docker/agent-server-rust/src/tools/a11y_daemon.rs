//! 按需启停 AT-SPI accessibility daemon（RECEIVE_ONLY 模式专用）。
//!
//! 稳态零 a11y（低 CPU）；仅登录等执行窗口期临时拉起 at-spi-bus-launcher，
//! 执行结束立即停掉，回到低 CPU 稳态。根治掉线后自动重登的 a11y 失效问题。

use crate::ia::types::Session;

/// at-spi-bus-launcher 可执行路径。
const AT_SPI_LAUNCHER: &str = "/usr/libexec/at-spi-bus-launcher";
/// pgrep/pkill 用的进程匹配子串。
const AT_SPI_MATCH: &str = "at-spi-bus-launcher";

/// at-spi-bus-launcher 是否在跑。
pub fn is_a11y_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-f", AT_SPI_MATCH])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

/// 确保 a11y daemon 活着；不在跑就以 session 的 linux_user 启动并等待注册。
pub async fn ensure_a11y_running(session: &Session) {
    if is_a11y_running() {
        return;
    }
    tracing::info!(
        "[a11y] on-demand start at-spi-bus-launcher (display={}, user={})",
        session.display,
        session.linux_user
    );
    let cmd = format!(
        "DISPLAY={} DBUS_SESSION_BUS_ADDRESS={} HOME=/home/{} {} &",
        session.display,
        session.dbus_address.as_deref().unwrap_or_default(),
        session.linux_user,
        AT_SPI_LAUNCHER
    );
    let _ = std::process::Command::new("su")
        .args(["-s", "/bin/bash", "-c", &cmd, session.linux_user.as_str()])
        .spawn();
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
}

/// 已废弃（2026-06-15）：曾在每次执行窗口结束 pkill at-spi-bus-launcher 省 CPU，但会销毁 a11y bus →
/// 长期掉线的 WeChat atk-bridge 注册永久失效、不重连 → 下次登录 a11y unavailable，反复人工救。
/// 现 a11y bus 起一次永不销毁（常驻 idle CPU ≈ 0），此函数保留为 no-op 仅兼容调用点。
pub fn stop_a11y() {
    // intentionally no-op：永不销毁 a11y bus，见上方说明
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_pattern_matches_launcher_path() {
        // at-spi-bus-launcher 进程匹配模式必须能命中其可执行路径
        assert!("/usr/libexec/at-spi-bus-launcher".contains(super::AT_SPI_MATCH));
    }

    #[test]
    fn test_is_a11y_running_does_not_panic() {
        // 不论环境有没有 at-spi，都应返回 bool 而非 panic
        let _ = super::is_a11y_running();
    }

    #[test]
    fn test_stop_guard_inactive_is_noop() {
        // active=false 时 drop 不应触碰系统（仅验证构造/析构不 panic）
        struct G {
            active: bool,
        }
        impl Drop for G {
            fn drop(&mut self) {
                if self.active {
                    super::stop_a11y();
                }
            }
        }
        let _g = G { active: false };
    }
}
