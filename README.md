# devctl

GitHub の repository を clone し、Dev Container を起動して Zellij セッションに
接続するまでをまとめた個人用 CLI。`gh` / `git` / `devcontainer` / `zellij` の
簡易ラッパー。

```bash
devctl open owner/repo
```

## 前提

以下がインストール済みで、`gh` が認証済みであること。

[`git`](https://git-scm.com/) /
[`gh`](https://cli.github.com/) /
[`docker`](https://docs.docker.com/) /
[`devcontainer`](https://github.com/devcontainers/cli) /
[`zellij`](https://zellij.dev/)

`devctl doctor` で一括確認できる。

コンテナエンジンは標準で Docker を使う。Podman は experimental 対応で、
管理ルートの `devctl.toml` に次を追加すると選択できる。

```toml
container_engine = "podman"
```

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

Claude Code / Codex向けのdev-box共通指示を `~/.claude/CLAUDE.md` と
`${CODEX_HOME:-$HOME/.codex}/AGENTS.md` へ配置する。

```bash
devctl agents install
```

repository はこの下に置かれる。

```
~/workspaces/
├── devctl.toml
└── projects/
    └── <owner>/
        └── <repo>/
```

`devctl.toml` は管理ルートの目印でもあり、以降どのサブディレクトリからでも
コマンドが効く。

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

## コマンド

| command | 動作 |
|---|---|
| `devctl init` | 管理ルートを初期化する |
| `devctl list` | 管理下の repository を一覧する |
| `devctl context [owner/repo]` | repository の主要情報を表示する |
| `devctl open [owner/repo]` | 必要なら clone し、コンテナ起動後 Zellij に接続する |
| `devctl ensure [owner/repo]` | 必要なら clone し、Dev Container を起動して終了する |
| `devctl up [owner/repo]` | Dev Container を起動する |
| `devctl exec [owner/repo] -- <cmd>` | コンテナ内でコマンドを実行する |
| `devctl shell [owner/repo]` | コンテナ内で bash を起動する |
| `devctl doctor` | 外部コマンド・GitHub 認証・選択したコンテナエンジンへの接続を診断する |
| `devctl agents install [--print]` | Claude Code / Codex向けのユーザー共通指示を配置する |

`context` / `list` / `ssh-target` / `doctor` は `--json` で機械可読なJSONを出力できる。
JSON利用時、stdoutにはJSONだけが出力される。

clone するのは `open` / `ensure` だけ。`up` / `exec` / `shell` は暗黙に repository を作らない。

`exec` は `--` 以降をそのまま子プロセスへ渡し、終了コードもそのまま返す。

```bash
devctl exec -- cargo test --release -- --nocapture
```

Zellij のセッション名は repository ごとに決まるので、翌日また `open` すれば
同じセッションに戻る。

## 仕様

[`docs/spec.md`](docs/spec.md)

## ライセンス

[MIT](LICENSE)
