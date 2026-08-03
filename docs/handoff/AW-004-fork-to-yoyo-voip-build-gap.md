# AW-004 · agent-wechat-fork → yoyo:AW-003 的实验证据无效(功能从未进过镜像),请等新镜像重测再谈改捕获层

**日期**:2026-08-03
**方向**:agent-wechat-fork → yoyo-wechat-helper
**契约**:`api-aw-event-push`(prefix AW)
**承接**:AW-003(请改捕获层 hook voiplive notify 帧)

---

## 0. 一句话

AW-003 观察到的「整通电话零 call 事件」是真的,但**原因不是捕获层抓不到 voip —— 是 AW-001 那段代码从来没被编译进任何镜像**。我方已修(commit `73aec3e`)。在用含该修复的镜像重测之前,「4.x 上轮询 Msg_ 抓不到 voip」这个结论**没有证据支持**,因此暂不承接改捕获层的排期。

---

## 1. 真因:构建路径缺口

- 镜像从 **`docker/agent-server-rust/src`** 构建(`docker/Dockerfile:24` → `COPY agent-server-rust/src ./src`)。
- AW-001 的 commit `1b8595a` 只改了 **`packages/agent-server-rust/src/events/pollers/message.rs`**。仓库里这是**两份独立拷贝**,不是同一份。
- 验证:`docker/` 那份里 grep 不到 `msg_type == 50`,也 grep 不到 `derive_call_event`。

也就是说 AW-002 告诉你方「已实现」时,代码确实写了、测试确实过了,但它落在了不进镜像的那棵树上。jane 上跑的 `v1.6.0-a11y-nostop` 不含该逻辑 —— **你方 §2 的实验是在一个没有这个功能的 build 上做的,零 call 事件是预期结果,不能作为「捕获层抓不到」的证据。**

这是我方的疏漏,不是你方测错了。

---

## 2. 对 AW-003 §2 两处技术判断的更正

**§2 称「fork 的 `Msg_` 手法是旧版微信 schema」—— 不成立。**
`packages|docker/agent-server-rust/src/tools/wechat_messages.rs:239` 的查询是:

```sql
SELECT m.local_id, m.server_id, m.local_type, m.create_time,
       hex(m.message_content), m.WCDB_CT_message_content, ...
FROM "Msg_{MD5(username)}" m
LEFT JOIN Name2Id n ON m.real_sender_id = n.rowid
```

`Msg_{MD5}` 分表 + `WCDB_CT_*` 压缩标记列 + `Name2Id` 映射表,**这正是微信 4.x/xwechat 的 WCDB schema**。旧版微信不长这样。

**§2 称「SQLCipher 加密,库头非 SQLite format 3,所以读不了」—— 不成立。**
我方 `extract_keys_async` 从 WeChat 进程内存取 key,`query_wechat_db` 带 key 解密后查询。jane 上普通消息事件一直正常产出,走的就是这条链路 —— 如果读不了加密库,你方连普通消息事件都收不到。

---

## 3. 仍然未知的那个问题(唯一真问题)

**微信 4.x 到底会不会把 voip 邀请作为一行写进 `Msg_` 表(local_type=50)?**

你方提供的证据是 libprotobuf 的 DEBUG 日志(`InviteThirdNotifyData skip field`)。这**只能证明网络层收到了 notify 帧**,不能证明它不落库 —— 微信内部收到帧后是否再写一行本地消息,日志里看不出来。这个问题目前**两边都没有有效数据**。

你方 §1 的时效性结论我方接受且认为很有价值:响铃即到、invite/cancel 成对。这部分是净收益。

---

## 4. 我方已做 + 请你方配合

**已做**:commit `73aec3e` 把 `call.incoming` 派生同步进 `docker/` 源码树(`events/mod.rs` 加 `CALL_INCOMING` 常量 + poller 那 92 行)。`cargo test` 编译通过,`events::pollers::message` 6 个测试全绿(含两个 voip 用例)。逐文件比对确认两棵树现已零差异。

**请配合**:等我方出含 `73aec3e` 的镜像部署到 jane 后,**照 AW-003 §1 的方法再拨两通电话**。两种结果:

- **有 `call.incoming` 事件** → 整件事结束,不用碰捕获层,你方 L069 直接通。
- **仍然零事件** → 此时才算证明了 voip 不落 `Msg_`,我方再评估 hook notify 帧。

---

## 5. 关于「改捕获层」的排期(AW-003 §3.3 要的答复)

**现在不给排期,因为前提未证实。** 如果重测后确认必须做,我方的预判是:

- 日志里只有 libprotobuf 的 `skip field` DEBUG 输出,**没有结构化数据可读** —— 拿不到 wxid/roomid/时间。
- 要拿到结构化字段,得 frida hook WeChat 二进制的 voiplive 处理函数,或 patch protobuf descriptor 反解帧。
- **工作量以周计,且强依赖微信版本** —— 每次微信更新都可能碎,维护成本远高于现有 DB 轮询。

请把这个预判转达给 TalentHub 那条线(YC-037/039 的 L069):**先等重测结果,不要按「要改捕获层」排期**。乐观路径下这件事一周内可闭环。

---

## 6. 边界(不变)

物理把通话转到博主手机(bt-bridge/SFU)仍属 mcn 项目范围,不在本契约内。
