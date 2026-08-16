# devctl

GitHub の repository を開発 VM 上で管理し、Dev Container と Zellij を使った
開発開始フローを薄く自動化する個人用 CLI。

```bash
devctl open owner/repo
```

これだけで clone・Dev Container の起動・Zellij セッションへの接続まで済む。

既存の CLI（`gh` / `git` / `devcontainer` / `zellij`）を順番に呼ぶだけの接着剤で、
workspace platform ではない。状態を持たず、daemon も DB も Web UI もない。

## 想定している開発スタイル

```
┌─ 開発VM (ホスト) ─────────────────────────┐
│  ソースコードの正本                        │
│  Zellij セッション                         │
│    └─ coding agent (Claude Code など)      │
│                                            │
│  ┌─ Dev Container ──────────────────┐      │
│  │  SDK / compiler / library        │      │
│  │  build / test                    │      │
│  └──────────────────────────────────┘      │
└────────────────────────────────────────────┘
```

ソースコードと coding agent はホストに置き、プロジェクト依存のツールチェインは
Dev Container に閉じ込める。agent はホストでファイルを直接編集し、
build や test だけを `devctl exec` でコンテナへ投げる。

ホスト VM を汚さずに複数プロジェクトを並行させるのが目的。

## 前提

以下がインストール済みで、`gh` が認証済みであること。

[`git`](https://git-scm.com/) /
[`gh`](https://cli.github.com/) /
[`docker`](https://docs.docker.com/) /
[`devcontainer`](https://github.com/devcontainers/cli) /
[`zellij`](https://zellij.dev/)

`devctl doctor` で一括確認できる。

## インストール

```bash
cargo install --git https://github.com/Gabuniku/devctl
```

## セットアップ

管理ルートにしたいディレクトリで `init` する。

```bash
mkdir -p ~/workspaces
cd ~/workspaces
devctl init
```

repository はこの下に置かれる。

```
~/workspaces/
├── devctl.toml
└── projects/
    └── <owner>/
        └── <repo>/
```

`devctl.toml` は管理ルートの目印でもある。以降どのサブディレクトリからでも
コマンドが効くようになる。

## 使い方

```bash
devctl list                      # 管理下の repository を一覧
devctl open owner/repo           # clone → コンテナ起動 → Zellij 接続
```

repository の中にいれば `owner/repo` を省略できる。

```bash
cd ~/workspaces/projects/owner/repo
devctl up                        # コンテナ起動
devctl exec -- cargo test        # コンテナ内で実行
devctl shell                     # コンテナ内 bash
```

| command | 動作 |
|---|---|
| `devctl init` | 管理ルートを初期化する |
| `devctl list` | 管理下の repository を一覧する |
| `devctl open [owner/repo]` | 必要なら clone し、コンテナ起動後 Zellij に接続する |
| `devctl up [owner/repo]` | Dev Container を起動する |
| `devctl exec [owner/repo] -- <cmd>` | コンテナ内でコマンドを実行する |
| `devctl shell [owner/repo]` | コンテナ内で bash を起動する |
| `devctl doctor` | 外部コマンド・GitHub 認証・Docker 接続を診断する |

clone するのは `open` だけ。`up` / `exec` / `shell` は暗黙に repository を作らない。

`exec` は `--` 以降をそのまま子プロセスへ渡し、終了コードもそのまま返す。

```bash
devctl exec -- cargo test --release -- --nocapture
```

Zellij のセッション名は repository ごとに決まるので、翌日また `open` すれば
同じセッションに戻る。

## `CLAUDE.local.md`

`devctl` は repository root に `CLAUDE.local.md` を置き、coding agent へ
環境の制約を伝える。

```markdown
- Source files are stored on the development VM. Edit them here.
- Project runtime commands must run inside the Dev Container.
- Use `devctl exec -- <command>`, e.g. `devctl exec -- cargo test`.
- Do not install project dependencies directly on the host VM.
```

共有される `CLAUDE.md` は repository 自身の情報に使い、個人環境に依存する情報は
こちらへ分ける。このファイルは共有 `.gitignore` を汚さず、ローカルの exclude で
無視されるので、他の開発者には見えない。

## 設計方針

- 外部 CLI の機能を再実装しない
- 状態を持たない。真実の源はファイルシステムと Git
- プロジェクト固有の build / test は各 repository の仕組みに任せる
- 依存 crate は `anyhow` / `clap` / `serde` / `toml` の 4 つのみ

GitLab や任意の Git URL への対応、worktree 管理、Zellij レイアウトの自動生成、
task runner といった機能は持たない。実装は「退屈なくらい薄い」状態を保つ。

設計の詳細は [`docs/spec.md`](docs/spec.md) を参照。

## ライセンス

[MIT](LICENSE)
