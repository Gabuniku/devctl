# devctl

GitHub の repository を常駐開発 VM 上で管理し、Dev Container と Zellij を使った
開発開始フローを薄く自動化する個人用 CLI。

```bash
devctl open owner/repo
```

これだけで「未 clone なら clone → `CLAUDE.local.md` 配置 → Dev Container 起動 →
Zellij セッションに attach」まで進む。

## これは何ではないか

workspace platform ではない。Coder や Codespaces のような仕組みを再実装するものではなく、
既存の CLI を順番に呼ぶだけの接着剤である。

- GitHub 操作は `gh` に委譲する（GitHub API を直接叩かない）
- Git の情報は `git rev-parse` に問い合わせる（`.git` を直接探索しない）
- コンテナ操作は `devcontainer` CLI に委譲する（Docker API を直接叩かない）
- セッション管理は `zellij` に委譲する（独自のセッション DB を持たない）
- build / test コマンドは各 repository の Justfile / Makefile / Cargo などに任せる

常駐 daemon も、SQLite も、独自 workspace DB も、Web UI も持たない。
状態は「ファイルシステム上にディレクトリがあるか」だけで、それも Git 自身に問い合わせる。

## 前提

以下がインストール済みで、`gh` は認証済みであること。

| | |
|---|---|
| [`git`](https://git-scm.com/) | Git 操作 |
| [`gh`](https://cli.github.com/) | GitHub 操作・認証 |
| [`docker`](https://docs.docker.com/) | Dev Container の実行基盤 |
| [`devcontainer`](https://github.com/devcontainers/cli) | Dev Container CLI |
| [`zellij`](https://zellij.dev/) | ターミナルセッション |

`devctl doctor` で一括確認できる。

## インストール

```bash
cargo install --git https://github.com/Gabuniku/devctl
```

または clone してビルドする。

```bash
git clone https://github.com/Gabuniku/devctl
cd devctl
cargo install --path .
```

## セットアップ

管理ルートにしたいディレクトリで `init` する。

```bash
mkdir -p ~/workspaces
cd ~/workspaces
devctl init
```

`devctl.toml` と `projects/` ができる。

```
~/workspaces/
├── devctl.toml          # projects_dir = "projects"
└── projects/
    └── <owner>/
        └── <repo>/
```

`devctl.toml` は設定ファイルであると同時に、**管理ルートを示す marker** でもある。
すでに存在する場合、`init` は上書きせずエラーで止まる。

## 使い方

```bash
devctl list                      # 管理下の repository を一覧
devctl open owner/repo           # clone → コンテナ起動 → Zellij attach
```

repository の中にいれば引数を省略できる。

```bash
cd ~/workspaces/projects/owner/repo
devctl up                        # コンテナ起動まで（clone はしない）
devctl exec -- cargo test        # コンテナ内で実行
devctl shell                     # コンテナ内 bash
```

### コマンド

| command | 動作 |
|---|---|
| `devctl init` | 管理ルートを初期化する |
| `devctl list` | 管理下の repository を `owner/repo` 形式で一覧する |
| `devctl open [owner/repo]` | 未 clone なら clone し、コンテナ起動後 Zellij に attach する |
| `devctl up [owner/repo]` | 既存 repository の Dev Container を起動する（clone はしない） |
| `devctl exec [owner/repo] -- <cmd>` | コンテナ内でコマンドを実行する |
| `devctl shell [owner/repo]` | コンテナ内で bash を起動する |
| `devctl doctor` | 外部コマンド・GitHub 認証・Docker 接続を診断する |

`open` だけが clone する。`up` / `exec` / `shell` は暗黙に repository を作らない。
副作用のある操作を予測可能にするためである。

`exec` は `--` 以降の引数を解釈せずそのまま子プロセスへ渡し、終了コードもそのまま返す。

```bash
devctl exec -- cargo test --release -- --nocapture
```

### repository 指定の省略

引数を省略すると `git rev-parse --show-toplevel` で現在の repository を求め、
それが管理ルート配下の `<projects_dir>/<owner>/<repo>` であることを確認して
`owner/repo` を逆算する。repository の外で省略した場合は明示的なエラーになる。

### 管理ルートの検出

1. current directory から親方向へ `devctl.toml` を探す
2. 見つからなければ `~/workspaces/devctl.toml` を探す
3. どちらも無ければエラー

repository のどのサブディレクトリからでもコマンドが効く。

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

ソースコードと coding agent はホスト側に置き、
プロジェクト依存の SDK やツールチェインは Dev Container に閉じ込める。
agent はホストでファイルを直接編集し、build / test だけを `devctl exec` でコンテナへ投げる。

ホスト VM を汚さずに複数プロジェクトを並行させることが目的である。

### `CLAUDE.local.md`

`devctl` は repository root に `CLAUDE.local.md` を生成し、
coding agent へ環境の制約を伝える。

```markdown
- Source files are stored on the development VM. Edit them here.
- Project runtime commands must run inside the Dev Container.
- Use `devctl exec -- <command>`, e.g. `devctl exec -- cargo test`.
- If the Dev Container is not running, start it with `devctl up`.
- Do not install project dependencies directly on the host VM.
- Do not run `devctl open`: it attaches an interactive Zellij session and will not return.
```

共有される `CLAUDE.md` は repository 自身の情報に使い、
個人環境に依存する情報はこちらへ分離する。

このファイルは共有 `.gitignore` を汚さず、
`git rev-parse --git-path info/exclude` で得たローカルの exclude に追記して無視される。

内容が期待値と一致していれば何もせず、異なれば devctl の管理内容で上書きする。
merge 機構は持たない。

### Zellij セッション

セッション名は `RepoId` から機械的に生成する。

```
owner/repo → dev-owner--repo
```

`zellij attach -c` を使うので、あれば attach、無ければ作成される。
devctl 側でセッション一覧を保持しない。

## 設計方針

- 外部 CLI の機能を再実装しない
- 状態を持たない。真実の源はファイルシステムと Git
- interactive なコマンドは独自 PTY を作らず、現在の stdio をそのまま継承する
- 純粋な path / parse / text 編集ロジックだけを切り出して unit test する。
  外部 CLI を mock する仕組みは作らない
- 外部コマンドの stderr は、失敗が正常系かどうかで抑制を決める。
  存在判定は捨て、成功を期待する操作は通す（診断情報が失われるため）

依存 crate は `anyhow` / `clap` / `serde` / `toml` の 4 つのみ。
async runtime も HTTP client も持たない。

設計の詳細は [`docs/spec.md`](docs/spec.md) を参照。

## 対象外

MVP では以下を実装しない。

GitLab / Gitea / 任意の Git URL、GitHub API・Docker API の直接利用、
devcontainer 設定のパース、workspace DB、daemon、Web UI、独自セッションマネージャ、
プロジェクト固有の build / test 管理、task runner、Zellij レイアウトの自動生成、
remote URL の厳密検証、worktree 管理、既存 repository の import、
依存の自動インストール、自動修復、plugin system、async / 並列処理。

`git worktree` を管理する機能は持たないが、将来の妨げにならないよう
Git root の取得は `git rev-parse`、Git 内部 path の取得は `git rev-parse --git-path` を使う。

## ライセンス

[MIT](LICENSE)
