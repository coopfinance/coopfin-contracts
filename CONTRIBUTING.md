# Contributing to CoopFinance

Thank you for contributing! CoopFinance participates in the **[Stellar Drips Wave](https://drips.network/wave/stellar)** and **[OnlyDust](https://onlydust.com)** programs — merged contributions may earn XLM rewards.

## How to Contribute

### 1. Find an Issue
- Browse [open issues](../../issues)
- Look for `good first issue`, `bounty`, or `help wanted` labels
- Comment on an issue to claim it **before starting work**

### 2. Fork & Branch
```bash
git checkout -b feat/your-feature-name   # new feature
git checkout -b fix/issue-123            # bug fix
git checkout -b docs/update-readme       # documentation
```

### 3. Make Changes
Follow the code style of the repo. Run lints and tests before pushing.

### 4. Submit a Pull Request
- Reference the issue number: `Closes #123`
- Describe what changed and why
- Add screenshots / test output if applicable

## Commit Convention

```
feat: add loan approval notification
fix: correct USDC decimal formatting
docs: update treasury contract reference
test: add voting contract edge case
chore: bump stellar-sdk to v12.3
```

## Code Standards

- TypeScript: strict mode, no `any`
- Rust: `cargo clippy` must pass with no warnings
- All new functions need tests
- Keep PRs focused — one concern per PR

## Getting Help

- Discord: [discord.gg/coopfinance](https://discord.gg/coopfinance)
- Open a discussion if unsure about an approach

## License

By contributing, you agree your code is licensed under MIT.

