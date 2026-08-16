# devctl MVP 仕様

Rustで `devctl` という個人用CLIを実装する。

目的は、GitHub上のrepositoryをローカルの常駐開発VM上で管理し、Dev ContainerとZellijを使った開発開始フローを薄く自動化すること。

Coderやworkspace platformの再実装はしない。

既存CLIを束ねるだけの小さな「接着剤」として実装する。

## 基本方針

以下の既存CLIを利用する。

* GitHub: `gh`
* Git: `git`
* Container: `devcontainer`
* Session: `zellij`
* Docker診断: `docker`

Rust側からGitHub APIやDocker APIを直接利用しない。

常駐daemon、SQLite、独自workspace DB、独自session管理、Web UIは不要。

プロジェクト固有のbuild/testコマンドも `devctl` では管理しない。

それらは各repositoryの以下のような仕組みに任せる。

* Justfile
* Makefile
* Taskfile
* package.json scripts
* Cargo

## 実行環境

ホスト環境は以下。

* Ubuntu 26.04 LTS
* user: `gabuniku`
* UID/GID: `1000/1000`
* Dockerは一般ユーザーからsudoなしで利用可能
* `gh` は認証済み

`git` / `gh` / `docker` / `devcontainer` / `zellij` / Claude Code / Codex CLI はすでにインストール済み。

Claude Code / Codex自身はホストVM上で動かす。

ソースコードの正本もホストVM上に置く。

Dev Containerはプロジェクト依存のSDK、compiler、library、runtime、build/test用途に使用する。

## workspace構造

管理ルートは次の構造にする。

```text
workspaces/
├── devctl.toml
└── projects/
    ├── owner-a/
    │   └── repo-a/
    └── owner-b/
        └── repo-b/
```

`devctl.toml` が存在するdirectoryを **devctl管理ルート** と定義する。

`devctl.toml` は単なる設定ファイルではなく、管理ルートを示すmarkerでもある。

初期仕様は最小限でよい。

```toml
projects_dir = "projects"
```

TOML解析には `serde` / `toml` を利用してよい。

## 管理ルート検出

管理ルートの解決は以下の順番。

1. current directoryから親directory方向へ `devctl.toml` を探す
2. 見つからなければ `~/workspaces/devctl.toml` を探す
3. それも存在しなければ明示的なerrorにする

例えば、

```text
~/workspaces/projects/Gabuniku/foo/src/
```

でコマンドを実行した場合、

```text
~/workspaces/devctl.toml
```

を発見し、

```text
~/workspaces
```

を管理ルートとして扱う。

管理ルートの検出処理は独立した関数にする。

将来的に `--root` や環境変数を追加できる設計にはしてよいが、MVPでは実装不要。

## repository形式

MVPではGitHubのみを対象とする。

repository指定は、

```text
owner/repo
```

形式だけ受け付ける。

例:

```text
Gabuniku/foo
```

repositoryを表す型を用意する。

概念例:

```rust
struct RepoId {
    owner: String,
    name: String,
}
```

`FromStr` 等でparseする。

最低限以下をvalidationする。

* `/` がちょうど1個
* ownerが空ではない
* repository名が空ではない
* `.` を拒否
* `..` を拒否

GitHub repository名の完全なvalidationをdevctl側で再実装する必要はない。

最終的な不正値は `gh` に判断させてよい。

## repository保存先

repositoryは、

```text
<workspace-root>/<projects_dir>/<owner>/<repo>
```

へcloneする。

例えば、

```text
~/workspaces/projects/Gabuniku/foo
```

未cloneの場合は、

```bash
gh repo clone Gabuniku/foo ~/workspaces/projects/Gabuniku/foo
```

相当を実行する。

GitHub操作は必ず `gh` CLIを利用する。

## clone済みrepository判定

directoryの有無だけではなく、Git repositoryとして有効か確認する。

`.git` directoryの存在を直接確認しない。

worktree等への将来的な互換性を壊さないよう、Git自身へ問い合わせる。

例:

```bash
git -C <path> rev-parse --is-inside-work-tree
```

MVPではremote URLが指定された `owner/repo` と一致するかまでは確認しなくてよい。

## repository引数の省略

repositoryを対象とするcommandでは、`owner/repo` 指定をoptionalにする。

repository指定がある場合はそのrepositoryを使用する。

repository指定がない場合は、current directoryが属するGit repositoryを使用する。

Git repository rootの取得には、

```bash
git rev-parse --show-toplevel
```

を使用する。

`.git` directoryを手動探索しない。

さらに、そのrepositoryが現在のdevctl管理ルートの、

```text
<root>/<projects_dir>/<owner>/<repo>
```

配下であることを確認する。

例えば、

```bash
cd ~/workspaces/projects/Gabuniku/foo/src
devctl exec -- cargo test
```

は、

```text
Git root:  ~/workspaces/projects/Gabuniku/foo
devctl root: ~/workspaces
RepoId:    Gabuniku/foo
```

として解決できるようにする。

repository外でrepository指定を省略した場合は、明示的なerrorにする。

## CLI仕様

MVPでは以下のcommandを実装する。

```text
devctl init

devctl open  [owner/repo]
devctl up    [owner/repo]
devctl shell [owner/repo]
devctl exec  [owner/repo] -- <command...>

devctl list
devctl doctor
```

CLI parserには `clap` deriveを使用する。

## `devctl init`

`init` は既存repository登録用ではない。

**devctl管理ルート初期化command** とする。

current directoryに以下を作成する。

```text
devctl.toml
projects/
```

初期 `devctl.toml`:

```toml
projects_dir = "projects"
```

すでに `devctl.toml` が存在する場合は **error で中断する**（`projects/` の作成も行わない）。

既存repositoryを管理下へ登録する機能はMVPでは不要。

## `devctl up`

既存repositoryのDev Containerを起動する。

```text
repository解決 → CLAUDE.local.mdを整備 → .git/info/excludeを整備 → devcontainer up
```

実際には、

```bash
devcontainer up --workspace-folder <absolute-repo-path>
```

相当を実行する。

`up` はrepositoryを勝手にcloneしない。repositoryが存在しない場合はerror。

## `devctl open`

日常利用の便利command。

```text
repository解決
→ 未cloneなら gh repo clone
→ CLAUDE.local.mdを整備
→ .git/info/excludeを整備
→ devcontainer up
→ Zellij sessionへattach/create
```

`open` のみ、repositoryが存在しなければcloneする。

repository指定を省略した場合は、current repositoryを使用する。

repository外で省略された場合はerror。

## `devctl exec`

例:

```bash
devctl exec Gabuniku/foo -- cargo test
```

repository内なら、

```bash
devctl exec -- cargo test
```

も可能にする。

内部では、

```bash
devcontainer exec --workspace-folder <absolute-repo-path> cargo test
```

相当を実行する。

`--` 以降の引数はそのまま子processへ渡す。

project固有commandについてdevctlは解釈しない。

exit statusも可能な範囲で呼び出し元へ反映する。

## `devctl shell`

例:

```bash
devctl shell Gabuniku/foo
```

repository内なら `devctl shell` も可能。

MVPでは、

```bash
devcontainer exec --workspace-folder <absolute-repo-path> bash
```

相当でよい。

shellを高度に自動判定する必要はない。

## stdio / interactive command

`exec`、`shell`、`zellij` 等のinteractive commandでは、独自PTYを実装しない。

Rustの子processに現在のstdioをそのまま継承させる。

```rust
Command::new(...)
    .stdin(Stdio::inherit())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit())
    .status()
```

でよい。

interactive動作のために出力をcaptureしない。

一方、`git rev-parse` 等、値を取得する必要があるcommandのみstdoutをcaptureする。

## Zellij

独自session DBは作成しない。

`zellij attach -c <session-name>` を使用し、

* sessionがあればattach
* なければ作成

とする。

session名は `RepoId` から機械的に生成する。

MVPでは以下を採用する。

```text
dev-<owner>--<repo>
```

例:

```text
Gabuniku/foo → dev-Gabuniku--foo
```

`owner` と `repo` の境界が分かるよう `--` を使用する。

高度なcollision対策やhash付与はMVPでは不要。

Zellijのsession一覧をdevctl側で保持・管理しない。

## CLAUDE.local.md

共有の `CLAUDE.md` はrepository自身に関する情報だけに利用する。

個人環境依存の情報は `CLAUDE.local.md` にdevctlが生成する。

repository rootに `CLAUDE.local.md` を配置する。

内容はdevctl管理対象とする。

例えば:

```markdown
<!-- Managed by devctl. Manual changes may be overwritten. -->

# Local development environment

- Source files are stored on the development VM. Edit them here.
- Project runtime commands must run inside the Dev Container.
- Use `devctl exec -- <command>`, e.g. `devctl exec -- cargo test`.
- If the Dev Container is not running, start it with `devctl up`.
- Do not install project dependencies directly on the host VM.
- Do not run `devctl open`: it attaches an interactive Zellij session and will not return.
```

このファイルの読み手はそのrepositoryで動くcoding agent自身である。
「知らないと行動を間違える」情報だけを載せ、agentの行動を変えない情報
（`devctl init` / `devctl doctor` / `devctl shell` の説明、devctl自体の紹介）は
載せない。全repositoryの毎session で消費されるcontextだからである。

MVPではmerge機構を作らない。

期待する内容と現在の内容を比較し、

* 同じなら何もしない
* 異なるならdevctl管理内容で上書き

でよい。

## `.git/info/exclude`

`CLAUDE.local.md` は共有 `.gitignore` へ追加しない。

repository localの `.git/info/exclude` 相当へ `CLAUDE.local.md` を追加する。

ただし `.git/info/exclude` というpathを直接結合してはいけない。

worktree等を考慮し、Git自身からpathを取得する。

```bash
git -C <repo> rev-parse --git-path info/exclude
```

取得したファイルを読み、`CLAUDE.local.md` という完全一致行がなければ末尾へ追加する。

既に存在する場合は何もしない。重複行を生成しない。

MVPでは専用コメントblock等の高度な管理は不要。

## Dev Containerが存在しないrepository

devcontainer configurationの仕様をRust側で再実装しない。

基本的には、

```bash
devcontainer up --workspace-folder <repo>
```

をそのまま実行し、失敗したらcommand failureとして扱う。

`.devcontainer/devcontainer.json` 等をdevctlが詳細にparseする必要はない。

ただしerror messageには、どのrepositoryに対する `devcontainer up` が失敗したか分かるcontextを付ける。

## `devctl list`

管理下のrepositoryを一覧する。

```text
$ devctl list
Gabuniku/foo
yattulab/qi-bot-rs
```

* `<root>/<projects_dir>` を2階層読み、`owner/repo` 形式で1行ずつ出力する
* そのまま `devctl open` へ貼れる形式にする（pathではなく `RepoId` の表記）
* Git repositoryとして有効なものだけを出す。判定は `is_git_worktree` を使う
  （clone途中で壊れた残骸やゴミdirectoryを除外するため）
* 出力順は安定させる（sort）
* 管理下に何も無ければ何も出力せず正常終了する

管理ルート検出を使うので、repositoryの中からでも実行できる。
`ls` と違い実行場所に依存しない点が、このcommandを持つ理由である。

Dev Containerの起動状態やgitのdirty状態は表示しない。
Dockerやgitへの問い合わせが増えて薄さが失われるため、MVPでは対象外とする。

### `open` / `up` は一覧を表示しない

repository指定を省略して管理ルート等で実行した場合も、一覧表示へfallbackしない。
`open` は副作用の大きいcommandであり、current directoryによって
「sessionを開く」と「一覧を出す」に分岐するのは危険だからである。

代わりにerror messageから `devctl list` へ誘導する。

```text
$ cd ~/workspaces && devctl open
Error: repository was not specified and the current directory is not in one
run `devctl list` to see managed repositories
```

## `devctl doctor`

MVPに含める。

以下のexternal commandが利用可能か確認する。

* `git`
* `gh`
* `docker`
* `devcontainer`
* `zellij`

可能ならversionも表示する。

さらに最低限、

```bash
gh auth status
docker info
```

相当を使い、

* GitHub CLIが認証されているか
* Docker daemonへ接続できるか

を確認する。

自動インストールや自動修復はしない。

例:

```text
git          OK
gh           OK
docker       OK
devcontainer OK
zellij       OK

GitHub auth  OK
Docker daemon OK
```

失敗した項目が分かる表示にする。

## external command wrapper

処理の大半は `std::process::Command` で実装する。

過剰なcommand abstractionは避ける。

最初から大規模な `trait CommandRunner` 体系やDI frameworkを作らない。

必要なら小さなhelperだけ作る。概念的には、

```rust
run_inherited(...)
capture_stdout(...)
```

程度で十分。

### stderrの扱い

外部コマンドのstderrを抑制するかは、**失敗が正常系かどうか**で決める。両者を揃えてはいけない。

* 失敗が正常系の判定 (`is_git_worktree`、doctorの各検査) は stderr を捨てる。
  未cloneのrepositoryを指定するたびに `fatal: cannot change to ...` が出るのは純粋なノイズで、
  devctl自身のエラーより先に表示されて原因を誤認させる。
* 成功を期待する操作 (`git_toplevel`) は stderr を通す。
  権限エラーやrepository破損の理由はgitのstderrにしか出ず、抑制すると
  `git failed: exit status: 128` しか残らず診断できなくなる。

`capture_stdout` (継承) と `capture_stdout_quiet` (破棄) を用意し、呼び出し側で選ぶ。

純粋なpath・parse・text編集logicはunit test可能な形へ分離する。

external CLI自体を大量にmockする仕組みはMVPでは不要。

## error handling

`anyhow` を利用してよい。

external command failureにはcontextを付ける。

例えば単に `command failed` ではなく、

```text
failed to start Dev Container for Gabuniku/foo
```

のように、何の操作で失敗したか分かるようにする。

子processが失敗した場合、そのexit statusが確認できるerrorにする。

不要な独自error hierarchyは作らない。

## dependencies

初期依存は極力少なくする。

```toml
[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
toml = "..."
```

必要性がないcrateは追加しない。特に以下は不要。

* GitHub API client
* Docker API client
* async runtime
* database
* web framework

`tokio` も不要。

## module構成

過剰に細分化しない。

```text
src/
├── main.rs      … dispatchのみ
├── cli.rs       … clap定義 / command / args
├── config.rs    … devctl.toml / workspace root検出 / config parse
├── repo.rs      … RepoId / owner/repo parse / repository path算出 / Git root取得 / clone
├── workspace.rs … repository解決 / CLAUDE.local.md / git exclude / devcontainer操作 / zellij session名
├── doctor.rs    … doctor command
└── command.rs   … external process実行helper
```

（`doctor.rs` は並行実装時のファイル衝突を避けるため分離している）

最初から `github.rs`、`docker.rs` 等を大量に作る必要はない。

## test

少なくとも以下のpure logicにはunit testを書く。

* `RepoId` parse成功
* `RepoId` parse失敗
* owner/repoからrepository path生成
* Zellij session名生成
* managed repository pathからRepoIdを復元するlogic
* `CLAUDE.local.md` expected contents
* `.git/info/exclude` への重複なし追加logic
* workspace root探索logicの可能な範囲

external CLIを本格的にmockする巨大test frameworkは作らない。

実装の各段階で以下を通す。

```bash
cargo fmt --check
cargo test
cargo clippy -- -D warnings
```

## 明示的に実装しないもの

MVPでは以下を実装しない。

* GitLab / Gitea / arbitrary Git URL
* GitHub API直接利用 / Docker API直接利用
* devcontainer仕様parser
* workspace DB / SQLite / daemon / Web UI
* 独自session manager
* project固有build/test管理 / task runner
* Zellij layout自動生成
* remote URL厳密検証
* worktree管理機能
* existing repository import機能
* automatic dependency install / automatic repair
* plugin system
* async処理 / parallel処理

git worktreeをMVPで管理する必要はない。

ただし将来の邪魔にならないよう、

* Git root取得は `git rev-parse`
* Git内部path取得は `git rev-parse --git-path`

を使用する。

## 設計上の重要事項

このCLIはworkspace platformではない。

理想的には `open` 自体も、既存の小さな処理を順番に呼ぶだけにする。

```rust
fn open(repo: Option<RepoId>) -> Result<()> {
    let repo = resolve_or_clone_repo(repo)?;
    ensure_local_claude(&repo)?;
    ensure_git_exclude(&repo)?;
    devcontainer_up(&repo)?;
    attach_zellij(&repo)?;
    Ok(())
}
```

実際の型や関数名は適切に設計する。

重要なのは、`devctl` 自体が状態を大量に保持したり、外部toolの機能を再実装したりしないこと。

実装は「退屈なくらい薄い」状態を維持する。

## 完了条件

以下が実際に動作すること。

```bash
mkdir -p ~/workspaces
cd ~/workspaces
devctl init
```

その後、

```bash
devctl open Gabuniku/some-repo
```

で、

1. repositoryがなければclone
2. `CLAUDE.local.md` 配置
3. local git exclude設定
4. `devcontainer up`
5. `zellij attach -c ...`

まで進むこと。

またrepository内から、

```bash
devctl up
devctl exec -- cargo build
devctl exec -- cargo test
devctl shell
```

が動作すること。

最後に `devctl doctor` で必要なexternal commandと認証・Docker接続状態を確認できること。
