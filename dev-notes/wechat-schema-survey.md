# WeChat 加密 SQLite Schema 调研（for poller 实施）

> 调研时间：2026-05-27
> WeChat 版本：xwechat 4.x（Linux Wine 镜像）
> 容器：yoyo-agent-wechat
> 登录账号：wxid_l0omukqe8nc522_9dc1

## 数据库位置

`/home/wechat/xwechat_files/<account>/db_storage/`

| DB 文件 | 路径 | 用途 |
|---|---|---|
| message_0.db | `message/message_0.db` | 消息（每聊天一张 `Msg_<md5>` 表）+ 系统消息历史 |
| biz_message_0.db | `message/biz_message_0.db` | 公众号 / 服务号消息 |
| contact.db | `contact/contact.db` | 联系人 + 群信息 + 好友请求（stranger） |
| session.db | `session/session.db` | 会话列表（chats） |
| 其它 | `general.db` `sns.db` 等 | 暂不用 |

## 解密 pragma 套装（必须全部）

```sql
PRAGMA key = "x'<hex_key>'";
PRAGMA cipher_page_size = 4096;
PRAGMA kdf_iter = 256000;
PRAGMA cipher_hmac_algorithm = HMAC_SHA512;
PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;
```

⚠️ 现有 `tools/wechat_db.rs::query_wechat_db` 只设了 `cipher_compatibility = 4`；实测 SQLCipher CLI 必须把 4 个 KDF/HMAC pragma 都显式设才能解密。Rust 实现应该是设了 cipher_compatibility=4 隐含了这些 — 确认它能跑。

## contact.db 表清单

```
biz_info                 contact                 contact_label
chat_room                chat_room_info_detail   chatroom_member
encrypt_name2id          name2id                 openim_acct_type
openim_appid             openim_wording          oplog
stranger                 stranger_ticket_info    ticket_info
```

### `contact` 表（46 行）— 真正的联系人 + 群

```sql
CREATE TABLE contact(
  id INTEGER PRIMARY KEY,
  username TEXT,                 -- wxid_xxx (私) / xxx@chatroom (群) / 系统账号如 fmessage/notifymessage
  local_type INTEGER,            -- 0=系统(notifymessage)，1=私聊，2=群？，3?  → 实测枚举
  alias TEXT,                    -- 微信号
  encrypt_username TEXT,
  flag INTEGER,                  -- bitfield，含 isInChatRoom 等
  delete_flag INTEGER,           -- 1 = 已删
  verify_flag INTEGER,           -- 公众号标识
  remark TEXT,                   -- 备注名（用户自定义）
  remark_quan_pin TEXT,
  remark_pin_yin_initial TEXT,
  nick_name TEXT,                -- 微信昵称
  pin_yin_initial TEXT,
  quan_pin TEXT,
  big_head_url TEXT,
  small_head_url TEXT,
  head_img_md5 TEXT,
  chat_room_notify INTEGER,
  is_in_chat_room INTEGER,
  description TEXT,
  extra_buffer BLOB,             -- protobuf 二进制
  chat_room_type INTEGER
);
```

**ContactDiffPoller 用法**：
```sql
SELECT username, nick_name, remark FROM contact
WHERE local_type = 1 AND delete_flag = 0;
```
- `local_type = 1` 过滤掉群 / 系统账号，只看私聊联系人
- diff = 比较前一次 snapshot 的 wxid set，新增/消失

### `stranger` 表（0 行当前）— 好友请求

Schema 同 `contact` 表（agent-wechat 用同一套 schema）：
```sql
CREATE TABLE stranger(id, username, local_type, alias, ..., nick_name, ...);
```

**关键发现**：当前 0 行，说明 stranger 表只存"已被处理过但还没加好友"的临时数据。真正的"新好友请求"到达后，会同时：
1. 写入 stranger 表
2. 在 message_0.db 创建 `Msg_<md5("fmessage")>` 表（如果还没有）
3. 在该 table 插入一条 type=37 消息（XML content 含 alias/nickname/scene/verify message）

**FriendRequestPoller 实现策略**：
- 主路径：watch `Msg_<md5("fmessage")>` 表（chatId 是 `fmessage`，md5 = `962df025475d4ce521192751df4ebbfc`），新 type=37 消息 → 解析 XML emit `contact.friend_request`
- 配合 `stranger` 表查 alias / nick_name 补全
- 当前没数据时跳过

### `chat_room` 表 — 群基本信息

```sql
CREATE TABLE chat_room(id, username, owner, ext_buffer);
```

### `chatroom_member` 表 — 群成员关系

```sql
CREATE TABLE chatroom_member(room_id INTEGER, member_id INTEGER);
-- room_id / member_id 是 contact.id 外键
```

**ChatroomMemberPoller 注**：
- 不直接看这张表（diff 太重，几百群 × 几百成员）
- 改派生路径：MessagePoller 看 Msg_ 里 `local_type=10000` 的系统消息，解析中文文案 emit `chatroom.member_joined/_left`
- 本表用于**填充事件 data**：当 emit member_joined 时，可顺手 SELECT chatroom_member 拿到当前成员快照

## message_0.db 表清单

```
DeleteInfo                            Name2Id                     
DeleteResInfo                         SendInfo                    
HistoryAddMsgInfo                     TimeStamp                   
HistorySysMsgInfo                     wcdb_builtin_compression_record
Msg_<md5(chat_id)>                    (每个聊天一张)
```

### `Msg_<md5>` 表

```sql
CREATE TABLE Msg_xxx(
  local_id INTEGER PRIMARY KEY AUTOINCREMENT,
  server_id INTEGER,                       -- int64！TS 端 string 化
  local_type INTEGER,                      -- 见 type 映射
  sort_seq INTEGER,
  real_sender_id INTEGER,                  -- → Name2Id.rowid 外键
  create_time INTEGER,                     -- Unix epoch 秒
  status INTEGER,                          -- 1=sent, 2=read, ...
  upload_status INTEGER,
  download_status INTEGER,
  server_seq INTEGER,
  origin_source INTEGER,
  source TEXT,                             -- 压缩 XML（含群成员上下文）
  message_content TEXT,                    -- 压缩或明文，内容 body
  compress_content TEXT,                   -- 压缩 flag
  packed_info_data BLOB,
  WCDB_CT_message_content INTEGER,         -- 是否压缩
  WCDB_CT_source INTEGER
);
```

**message_content 编码**：
- `WCDB_CT_message_content = 1` → ZSTD 压缩
- `WCDB_CT_message_content NULL` → 明文
- 解压后是文本（type=1）或 XML（type=3/47/49/10000 等）

**MessagePoller 实现策略**：
```sql
SELECT local_id, server_id, local_type, create_time,
       hex(message_content) AS hex_content, WCDB_CT_message_content,
       n.user_name AS sender_name
FROM Msg_<md5> m
LEFT JOIN Name2Id n ON m.real_sender_id = n.rowid
WHERE m.local_id > ?    -- last_seen
ORDER BY m.local_id ASC
LIMIT 100;
```

### `local_type` 值映射（部分实测，部分参考）

| local_type | 含义 |
|---|---|
| 1 | text |
| 3 | image |
| 34 | voice |
| 37 | friend request（在 `Msg_<md5("fmessage")>` 里）|
| 43 | video |
| 47 | emoji sticker |
| 49 | appmsg（链接 / 引用回复 / 小程序 / ...）|
| 50 | voip 通话 |
| 10000 | 系统消息（群成员变化 / 改名 / 拍一拍 / 红包提醒 ...）|

⚠️ 实际看到 `local_type & 0x7FFFFFFF` 用法 — 高位 sign bit 要清掉再判等

### `HistorySysMsgInfo` — 系统消息历史索引

```sql
CREATE TABLE HistorySysMsgInfo(
  session_name_id INTEGER,
  history_id INTEGER,
  server_id INTEGER,
  is_revoke INTEGER,                       -- 是否撤回
  CONSTRAINT _UNIQUEID PRIMARY KEY(session_name_id, history_id, server_id)
);
```

**v2 用**：`is_revoke=1` 行就是被撤回的消息。`message.recalled` 事件可以从这里 poll。本期不做，留 v2。

### `Name2Id` 表 — 发送者映射

`real_sender_id` JOIN 到 `Name2Id.rowid` 拿 `user_name`（实际是 wxid）

## chatroom 系统消息（type=10000）文案样本

⚠️ **未实测样本**：当前测试群暂无成员变更事件历史。Codex 实施 ChatroomMember 派生 logic 时需要做的事：
1. 在 fork 部署后，**真实操作**：拉一个测试号进群 / 让其退群 / 改群名 / 拍一拍
2. 抓 type=10000 的 content 实际文本
3. 用文案 patterns 写正则匹配

参考 mac WeChat 文案（仅参考，Linux Wine 版可能略不同）：
- 邀请加入：`"<inviter>邀请<wxid>加入了群聊"` / `"<wxid>通过扫描你分享的二维码加入群聊"`
- 退群：`"<wxid>退出了群聊"`
- 被踢：`"你被群主移出了群聊"` / `"群主已经把<wxid>移出群聊"`
- 改群名：`"<wxid>修改群名为'xxx'"`
- 群主变更：`"<wxid>已成为新群主"`

**Phase 5 / Phase 10 时实测后回填本节**。

## fmessage（好友请求消息）XML 样本

⚠️ **未实测样本**：当前 stranger 表是空的。Phase 10 阶段加好友测试时抓样本。

预期 XML 结构（参考 mac WeChat）：
```xml
<msg fromusername="wxid_xxx" alias="user_alias" 
     encryptusername="..." fromnickname="昵称" 
     content="我是xxx" scene="14" ... />
```
- `scene` 值：14=群聊添加，17=名片，... 待实测
- `content` 是用户填的"打招呼"

## Codex 实施 reference

| Poller | 直接复用 | 自己写 SQL |
|---|---|---|
| MessagePoller | `crate::tools::wechat_messages::list_messages` | 不写 SQL，直接调 |
| FriendRequestPoller | `crate::tools::wechat_db::query_wechat_db` | `SELECT * FROM Msg_962df025... WHERE local_id > ?` + XML parse |
| ContactDiffPoller | `crate::tools::wechat_db::query_wechat_db` | `SELECT username, nick_name, remark FROM contact WHERE local_type=1 AND delete_flag=0` |
| 群成员变更（派生） | 在 MessagePoller 里判 type=10000 | 不另起 poller |

## 验收

- [x] contact.db 解密成功
- [x] message_0.db 解密成功
- [x] 关键表 schema dump 完整
- [x] 找到好友请求的存放位置（fmessage chat 的 Msg_ 表 + stranger 备用）
- [x] type 字段映射部分补齐
- [ ] 群系统消息文案样本（Phase 10 实测时补）
- [ ] friend request XML 样本（Phase 10 实测时补）
