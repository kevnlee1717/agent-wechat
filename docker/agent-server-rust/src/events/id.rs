//! 事件 ID 生成器。
//! 使用 ULID 保证事件 ID 的时间前缀单调递增，便于 since 游标比较。

use ulid::Ulid;

pub fn next_event_id() -> String {
    format!("evt_{}", Ulid::new().to_string())
}
