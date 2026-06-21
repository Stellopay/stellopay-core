## 关联 Issue
- https://github.com/Stellopay/stellopay-core/issues/510

## 改动摘要
- 为 `rate_limiter` 的 `set_limit_for` 增加正值校验，拒绝 `burst=0` 和 `refill_rate=0`。
- 补充回归测试，覆盖零值拒绝与正值通过两类路径。
- 同步更新 `docs/rate-limiter.md` 中的 API 说明。

## 验证方法
- 已人工核对 `onchain/contracts/rate_limiter/src/lib.rs`、`onchain/contracts/rate_limiter/tests/test_rate_limit.rs`、`docs/rate-limiter.md` 的一致性。
- 本地 Rust 工具链不可用，因此跳过 `cargo test -p rate_limiter` 和 `cargo fmt --check -p rate_limiter`。

## 改动文件
- `onchain/contracts/rate_limiter/src/lib.rs`
- `onchain/contracts/rate_limiter/tests/test_rate_limit.rs`
- `docs/rate-limiter.md`

## 风险说明
- 变更只收紧了一个管理员配置入口的输入范围，不影响正常正值配置。
- 新测试直接覆盖边界条件，能防止零值配置悄悄落地。

## 执行边界
- 没有改动其他合约逻辑。
- 没有触碰资金、密钥或外部服务。
- 没有重构与 issue 无关的代码。
- 本地 Rust 工具链缺失，因此不做伪验证。

## 安全边界
- 只涉及参数校验与单元测试。
- 未引入链上转账、签名、KYC 或私钥相关操作。

## 钱包地址
- RTC269fa5650798c3aa5086a128c025a546e0a41d0b