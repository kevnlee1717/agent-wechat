# agent-server-rust 源码地图（for WS push 改造）

> 调研时间：2026-05-27
> 调研人：Claude（Explore agent）
> 目的：在改造 WS push 前先把现有架构摸清，省得 Codex 撞墙

## 启动 / 状态管理总览

- **入口**: `src/main.rs:16-66` — `#[tokio::main]` async fn
- **没有 Axum AppState** — 全部状态走全局 static `OnceLock`
  - `db::DB: OnceLock<Mutex<Connection>>` (`src/db/mod.rs:10`) — agent 自己的 SQLite 连接
  - `sessions::manager::get_session(id)` — 从 DB 读 Session
  - `router::auth::init_token()` (`main.rs`) — 全局 token
- **现有 spawn 的 tokio task**: `sessions::health_monitor::spawn_health_monitor()`

**EventBus 安置方式**：跟 `db::DB` 同样 pattern，新增 `events::BUS: OnceLock<Arc<EventBus>>`，在 `main.rs` 初始化后调 `BUS.set(...)`。无需改 Router signature。

## Router 注册

- `router::build_router() -> Router` (`router/mod.rs:24-73`)
- 中间件栈：CORS → DefaultBodyLimit(50MB) → auth_middleware
- WS 路由当前是 `/api/ws/events → events::events_ws`

## 关键文件

| 文件 | 当前作用 | 改造方向 |
|---|---|---|
| `router/events.rs` | 19 行空 stub，loop recv | 改写为完整 WS 订阅 handler |
| `router/messages.rs:31-80` | `list_messages` REST handler | 不动；MessagePoller 复用同样的 `tools/wechat_messages::list_messages` |
| `tools/wechat_messages.rs` | 解密查 Msg_ 表 | 不动；Poller 直接调 |
| `tools/wechat_chats.rs` | 列群 / 私聊 | 不动；MessagePoller 用它枚举 active chats |
| `tools/wechat_db.rs:11-86` | `query_wechat_db(path, hex_key, sql)` 通用解密查询 | 不动；FriendRequestPoller / ContactDiffPoller 直接调 |
| `tools/wechat_keys.rs` | `extract_keys_async(pid)` 提密钥 | 不动；Poller 用 `get_stored_keys()` 先读缓存 |
| `sessions/manager.rs:98/156/234/267` | session FSM 状态点 | 在这些点调 `events::publish(Event::Session...)` |
| `db/queries.rs` | `update_session_logged_in_user` 等 | 在 update 后调 publish |
| `plans/login.rs` / `plans/logout.rs` | 登录登出步骤 | 在 logged_in_user 设值/清空处 publish |

## Message struct（`src/ia/types.rs:412-438`）

```rust
pub struct Message {
    pub local_id: i64,
    pub server_id: i64,
    pub chat_id: String,
    pub sender: Option<String>,
    pub sender_name: Option<String>,
    pub msg_type: i32,
    pub content: String,
    pub timestamp: String,
    pub is_mentioned: Option<bool>,
    pub is_self: Option<bool>,
    pub reply: Option<ReplyInfo>,
}
```

## WeChat 加密 SQLite 访问路径

1. `find_wechat_pid()` → PID
2. `find_account_dir(pid)` (`wechat_db.rs:119-141`) — scan `/proc/{pid}/fd` 找 `xwechat_files/{account}/db_storage/*.db`
3. `list_account_dbs(account_dir)` — 枚举 `message_*.db`, `contact.db`, `session.db`, etc.
4. `get_stored_keys(&db, session_id, account_dir)` — 从 agent DB `wechat_keys` 表读 hex_key
   - 若缓存空：`extract_keys_async(pid)` 调 Python 脚本 `/opt/tools/extract-keys.py`，输出 `/tmp/wechat_keys_{pid}.json`
5. `query_wechat_db(db_path, hex_key, sql) -> Vec<serde_json::Value>` 执行解密查询

## 需要新加 deps（Cargo.toml）

当前已有：tokio, axum 0.8 (ws feature), rusqlite (bundled-sqlcipher-vendored-openssl), serde, serde_json, async-trait, tokio-tungstenite

新增：
- `ulid = "1.1"` — 事件 ID
- `parking_lot = "0.12"` — RwLock for ring buffer
- `chrono = { version = "0.4", features = ["serde"] }` — 时间戳（如果还没有）

## Hook 点 cheat sheet（SessionLifecycleEmitter 用）

| 事件 | Hook 位置 | 信号 |
|---|---|---|
| session.login | `plans/login.rs` 调 `update_session_logged_in_user` 之后 | wxid 拿到 |
| session.logout | `plans/logout.rs` 同上 | wxid 之前的值 + reason="user_logout" |
| session.qr_refresh | login flow 里 QR generation step | qr_payload string |
| session.scan | login flow 里 QR scanned / confirmed callback | stage + wxid? |

**注意**：本笔记的精确行号是 Explore agent 给出的，Codex 实施时再确认。
