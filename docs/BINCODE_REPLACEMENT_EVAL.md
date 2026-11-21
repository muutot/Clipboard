# bincode 替代品评估：wincode / postcard / rkyv

评估日期：2026-11-21。基线：v1.8.5 工作树（`bincode 2.0.1`），本地 rustc 1.99.1。
本文是一次性决策证据，不是 wire 规范；v1 线格式以 [`SYNC_V1.md`](SYNC_V1.md) 与
`src-tauri/crates/clipboard-sync/src/v1/wire/` 源码为准。

## 结论

**维持 `bincode = "2"`（解析到 2.0.1），不迁移到 3.0.0，也不换成三个替代库。**

bincode 3.0.0 是停止维护的墓碑版本（crates.io 上 lib.rs 只有一个编译期错误，README
建议迁往 wincode / postcard / rkyv）。但 `bincode = "2"` 是 caret 约束，永远不会解析到
主版本 3.0.0，因此项目对毒版本天然免疫——「不升」是正确处置，不是妥协。

对三个替代品逐一评估后，**没有一个是 drop-in 替换**。只有 wincode 在付出显著迁移成本后
够得着逐字节兼容；postcard 与 rkyv 会破坏冻结数据和 anti-DoS 保证。下表为摘要。

| 候选                  | 与 bincode 2 `standard()` wire 兼容 | `with_limit` 式分配上限         | 判定             |
| --------------------- | ----------------------------------- | ------------------------------- | ---------------- |
| bincode 2.0.1（现状） | —                                   | 有                              | 维持             |
| wincode 0.6.3         | 仅自定义配置下逐字节一致            | 有（两套独立限额）              | 唯一可行但成本高 |
| postcard 1.1.3        | 否（LEB128 vs 251–254 标记）        | 无等价机制                      | 不适合           |
| rkyv 0.8.18           | 否（zero-copy 归档格式）            | 仅靠 bytecheck，历史有 OOB 公告 | 最不适合         |

## 为什么这是一个协议决策，而非依赖替换

`clipboard-sync` 里的 bincode 不是通用序列化，而是冻结的 v1 线协议。四条硬约束来自源码：

1. **字段顺序冻结**。DTO 注释明确写 "frozen bincode field order"
   （`src-tauri/crates/clipboard-sync/src/v1/wire/types.rs:1`）。未来本地 schema 迁移不得
   静默改变这个位置布局（见 `SYNC_V1.md`「Wire objects」）。
2. **内容寻址**。S3 对象的 nonce 从已认证 header 与压缩明文派生，保证同一逻辑对象重试
   产生逐字节相同的密文与 object key（`wire/crypto.rs` `encode_value`）。换序列化格式会
   改变字节 → 改变压缩结果 → 改变 SHA-256 → 所有历史 object key 失配。
3. **anti-DoS 分配上限**。`wire/mod.rs::decode_bounded_bincode` 用 const-generic 预算阶梯
   （`with_limit::<N>()`，按已验证原始字节长度 × 32 放大、64 KiB 下限、1 GiB 上限）阻止
   「小信封声明巨大字符串 / Vec」。专测：
   `wire/tests.rs::tiny_bincode_envelopes_cannot_claim_large_string_allocations`。
4. **framing 与大小预判**。`decode_from_slice` 返回 `(T, 消费字节数)`，同一 buffer 内连续
   解码依赖它；`packs.rs::mutation_batch_encoded_size` 用 `EncoderImpl::new(SizeWriter, …)`
   在写 chunk 前先算编码字节数。

任何替代库必须同时满足这四条，否则不是等价迁移。

## 候选逐一评估

### wincode 0.6.3 — 唯一够得着，但要做成一次协议迁移

- 维护：Anza（Solana/Agave 团队）出品，2026-10 仍在发版；经 Neodyme、OtterSec、
  Asymmetric Research 三家独立审计，并用 fuzz 做与 bincode 的 roundtrip 验证；MSRV 1.89，
  edition 2024，Apache-2.0。RUSTSEC-2025-0141（bincode 停止维护）将其列为首选替代之一。
- 字节兼容：**默认配置不兼容**。wincode 默认 `FixInt`，对齐 bincode 1.x / bincode 2
  `config::legacy()`；本项目用的是 `config::standard()`（`VarInt`）。README 的
  "bincode-compatible" 指前者，不能照字面相信。要复刻 `standard()` 必须显式构造
  `Configuration<true, LIMIT, BincodeLen, LittleEndian, VarInt, u32>`（varint 的
  251/252/253/254 标记法与 zigzag 与 bincode 2 一致，已有 `istmo-core` 实战验证），
  **并用真实历史 v1 数据做逐字节 golden 回归**。
- 限额：`with_preallocation_size_limit`（默认 4 MiB，需对大 pack 调高）与
  `with_deserialization_size_limit`（默认关闭，最接近 bincode `with_limit`），两套独立机制。
- 缺口：derive 换成 `SchemaWrite` / `SchemaRead`（带 `Src`/`Dst` 关联类型），
  `deserialize` 不返回消费字节数（需用 `Cursor::position()` 或 `&mut &[u8]` 长度差自行算），
  有 `serialized_size()` 可直接替代 `SizeWriter`。
- 判定：技术上可做到逐字节兼容且维护健康，但这是一次协议 breaking change：
  golden 测试 + bincode 只读双读过渡 + 全部调用点改写。可作为「未来确实要动协议时」的
  首选路径，现在不为换依赖而做。

### postcard 1.1.3 — 不适合

- 维护活跃、no_std、wire 自 1.x 起稳定，这些是加分项；`ser_flavors::Size` 与
  `experimental::serialized_size` 能满足「只算大小」，`take_from_bytes` 能拿消费字节数。
- 阻断项：① wire 用 LEB128，与 bincode 的 251–254 标记法不同，历史数据无法直接读，无
  官方转换器；② **没有 `with_limit::<N>()` 的等价机制**——已知 issue #135 中 Vec 声明长度
  被破坏会导致远超实际数据的预分配，正是本项目专测要防的情形；③ derive 换成 serde，
  且 `flatten` / internally-tagged enum 有已知坑；④ 无全局 fixed-int 切换，只有逐字段
  `postcard::fixint`（不支持 `usize`/`isize`）。
- 判定：不匹配「冻结持久化 + anti-DoS」的形态。

### rkyv 0.8.18 — 最不适合

- 活跃、高性能、no_std，但它是 zero-copy 归档格式，wire 与 bincode 完全不兼容。
- 阻断项：① 官方 feature-comparison 明确 "Schema evolution: no"，无兼容层或迁移路径；
  ② 高层 API（`access` / `from_bytes`）不返回消费字节数，framing 要改底层 API；
  ③ 安全记录：RUSTSEC-2026-0234（hash table 校验不足，crafted archive 可致越界读），
  影响 `<0.8.17`，若采用必须 ≥0.8.17；④ 磁盘/S3 读出的 `Vec<u8>` 可能未对齐，归档格式
  有对齐与 endian/pointer-width 的格式控制 feature，改动会让旧数据不可读。
- zero-copy 的价值在「本地 mmap 只读热查询」，本项目是跨网络传输且要 owned `T`，发挥不出优势。

## 复查触发条件

出现以下任一情况时重新评估，而不是沿用本文结论：

- v1 冻结 wire 或内容寻址语义被正式改为 v2，且历史数据有明确迁移策略；
- bincode 2 出现必须修复的安全公告且上游无补丁；
- 需要把 `clipboard-sync` 做成 no_std 目标（此时 postcard / wincode 的 no_std 优势才相关）。

## 相关文档

- [`SYNC_V1.md`](SYNC_V1.md) — v1 线格式、bincode 编码 / zstd 压缩 / 加密 envelope。
- [`DEPENDENCY_REACHABILITY.md`](DEPENDENCY_REACHABILITY.md) — 依赖公告可达性复核。
