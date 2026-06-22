# STELLOPAY-500 cargo-deny supply-chain gate

## 计划
1. 创建/确认分支 `ci/cargo-deny-supply-chain`
2. 编写 `onchain/deny.toml`，包含 `advisories`、`licenses`、`bans`、`sources`
3. 修改 `.github/workflows/security-scan.yml`，加入 blocking `cargo deny check`，移除 `|| true`
4. 更新 `docs/ci.md` 记录供应链门禁策略
5. 本地验证：`cargo deny check`，并检查 workflow / doc diff
6. 自检、提交 PR、提交 claim

## 验证
- `cargo deny check` passes locally
- workflow YAML valid
- security scan steps no longer use `|| true`
- docs explain dependency policy
