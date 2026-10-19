# macOS GUI / CLI 发布

仅发布 macOS 的 `aarch64-apple-darwin`（ARM64 / Apple Silicon）和 `x86_64-apple-darwin`（AMD64 / Intel）版本。不构建 Windows 或 Linux 宿主版本。

## 自动发布

工作流：`.github/workflows/release.yml`。

1. 修改 `Cargo.toml`、`desktop/package.json`、`desktop/src-tauri/Cargo.toml`、`desktop/src-tauri/tauri.conf.json` 的版本，并同步两个 Cargo.lock 内项目自身的版本。依赖版本无需变更。
2. 更新 README 的版本和更新说明。
3. 执行 `python3 scripts/version.py --tag v0.10.7`（替换为本次版本），提交并推送到 main。
4. 推送对应标签：

   ```sh
   git tag -a v0.10.7 -m 'VNet Studio 0.10.7'
   git push origin v0.10.7
   ```

标签必须与全部版本文件一致。推送 main 或创建 PR 只验证和上传 Actions 构建产物，不发布正式版本。推送 `v*` 标签才自动发布。也可从 Actions 的 **macOS build and release → Run workflow** 输入已有版本标签重试。两个架构都成功后，发布步骤检查四份资产、生成 `SHA256SUMS.txt`，上传至草稿并发布。预发布版本（如 `0.11.0-beta.1`）标记为 prerelease，不替换 Latest。

工作流使用 `macos-15` ARM runner 与 `macos-15-intel` Intel runner，原生编译并执行测试。构建最低部署目标为 macOS 11；CI 在 macOS 15 上验证，未验证所有较旧系统。发布仅需要内置 `GITHUB_TOKEN` 的 `contents: write` 权限，不需要额外 PAT。仓库需启用 GitHub Actions，并允许发布 job 获得写权限。

发布失败后可重跑失败 job。仍为草稿的版本会继续上传；已发布版本不会被覆盖，需要修复时发布新版本号。

## 发行文件

- `VNet-Studio-VERSION-macOS-arm64.zip`
- `VNet-Studio-VERSION-macOS-x86_64.zip`
- `vnetctl-cli-VERSION-macOS-arm64.tar.gz`
- `vnetctl-cli-VERSION-macOS-x86_64.tar.gz`
- `SHA256SUMS.txt`

GUI ZIP 保留 `.app` 结构，包含 `vnetctl` GUI 和同架构的 `vnetctl-cli`；CLI 压缩包保留可执行权限，并包含许可证与安装说明。CLI 的 `--version` 和 Mac 后台版本使用同一 Cargo 版本，应用 Info.plist 从统一版本检查结果生成。

产物采用 ad-hoc 签名，没有 Apple Developer ID 签名或公证。正式 Developer ID 签名需要后续单独配置 Apple 凭据。

下载所有资产后可校验：

```sh
shasum -a 256 -c SHA256SUMS.txt
```

只下载一个架构时，可从校验文件中选取对应文件名验证。

## 本地构建

需要 macOS、Xcode Command Line Tools、Rust stable、Node.js 22、pnpm 10.18.0（由 `desktop/package.json` 固定）、Python 3.11+（版本元数据检查）。Unix 来宾运行时仍只需 Python 3.9+。

```sh
# 当前 Mac 的原生架构，同时生成 GUI ZIP 与独立 CLI tar.gz
bash scripts/build-release.sh

# 仅生成桌面应用，默认输出到 ../VNet Studio.app
bash build-desktop.sh
```

发行压缩包输出到 `dist/`。可设置 `VNET_BUILD_TARGET` 指定目标，须先安装对应 Rust target；发行验证会执行 CLI，因此跨架构构建需要对应执行支持，推荐使用工作流的两个原生 runner。可设置绝对或相对 `CARGO_TARGET_DIR` 复用构建缓存；GUI 和 CLI 均从对应目标目录打包。

## 验证范围

每个架构独立执行 CLI 的 Rust 测试、Unix 来宾 Python 测试、Vue 类型检查和生产构建、Tauri 与 CLI release 构建、Mach-O 架构核对、CLI `--help` / `--version` 启动验证及 ad-hoc 签名验证。验证与打包不启动 Mac 转发后台或虚拟机。

GitHub runner 没有真实 Parallels 来宾；网络配置、VPN、转发及不同来宾系统的端到端功能仍需在有 Parallels 的 Mac 上验证。
