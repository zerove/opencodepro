//! 记录「这条质量指标是哪个引擎版本产出的」。
//!
//! 为什么需要：`docs/benchmarks/quality-metrics/records/` 是扁平样本目录，
//! 不同引擎版本产出的记录会混在一起聚合。没有版本标识时，「成功率 0.857」
//! 究竟反映哪个引擎版本**无法归因** —— 修掉一个缺陷后重跑，成功率变化也可能
//! 只是样本构成变了（而不是引擎变好/变坏）。
//!
//! 取的是**引擎源码仓库**的短 commit，不是任务工作区的：`dev`/`verify` 的
//! cwd 是用户/场景的工作区，从那里 `git rev-parse` 会拿到另一个仓库的版本
//! （或直接失败），那比未知更有害 —— 它会给出一个**看起来有效但其实无关**的
//! 版本号。

use std::path::Path;
use std::process::Command;

/// 编译时记录的引擎源码目录。
///
/// 用它而不是 cwd：见模块文档。分发的二进制装到别的机器上时该目录不存在，
/// 此时如实返回 `None`（未知 ≠ 猜一个）。
const ENGINE_SOURCE_DIR: &str = env!("CARGO_MANIFEST_DIR");

/// 引擎版本：commit + 工作区是否脏。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineRevision {
    /// 短 commit（7–40 位十六进制）。
    pub commit: String,
    /// 工作区是否有未提交改动。
    ///
    /// 必须有这个维度：脏工作区产出的记录**不是** `commit` 那个版本的代码跑出来的。
    /// 只写 commit 会让指标看起来可归因，实际是错的 —— 那比「未知」更有害。
    pub dirty: bool,
}

impl EngineRevision {
    /// 用于展示/分组的标签：脏版本带 `+dirty` 后缀。
    pub fn label(&self) -> String {
        if self.dirty {
            format!("{}+dirty", self.commit)
        } else {
            self.commit.clone()
        }
    }
}

/// 当前引擎版本，取不到则为 `None`。
pub fn engine_revision() -> Option<EngineRevision> {
    engine_revision_at(Path::new(ENGINE_SOURCE_DIR))
}

/// `engine_revision` 的可测版本。
pub fn engine_revision_at(engine_dir: &Path) -> Option<EngineRevision> {
    if !engine_dir.is_dir() {
        return None;
    }
    let commit = git_short_sha(engine_dir)?;
    Some(EngineRevision {
        commit,
        dirty: git_is_dirty(engine_dir),
    })
}

fn git_short_sha(engine_dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(engine_dir)
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?;
    normalize_sha(&sha)
}

/// 工作区是否脏。取不到状态时**按脏处理**：宁可标注「不确定」，
/// 也不要声称记录精确对应某个 commit。
fn git_is_dirty(engine_dir: &Path) -> bool {
    let Ok(output) =
        Command::new("git").arg("-C").arg(engine_dir).args(["status", "--porcelain"]).output()
    else {
        return true;
    };
    if !output.status.success() {
        return true;
    }
    match String::from_utf8(output.stdout) {
        Ok(status) => !status.trim().is_empty(),
        Err(_) => true,
    }
}

/// 校验并规范化 `git rev-parse --short` 的输出。
///
/// 只接受 7–40 位十六进制：`git` 出错时也可能往 stdout 写东西（例如
/// `--short` 遇到非 git 目录会往 stderr 写，但包装脚本/别名可能不同），
/// 把任意字符串当版本号写进记录会让「版本」失去意义。带 `-dirty` 后缀的
/// 输出（`--short=12 --dirty`）在这里不被接受 —— 调用方没要求 dirty 标记。
fn normalize_sha(raw: &str) -> Option<String> {
    let sha = raw.trim();
    let ok = (7..=40).contains(&sha.len()) && sha.chars().all(|c| c.is_ascii_hexdigit());
    ok.then(|| sha.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_short_and_full_shas() {
        assert_eq!(
            normalize_sha("5869d3dbae51\n").as_deref(),
            Some("5869d3dbae51")
        );
        assert_eq!(
            normalize_sha("5869d3dbae5165f2f525d97f9c74ab590084a405").as_deref(),
            Some("5869d3dbae5165f2f525d97f9c74ab590084a405")
        );
        assert_eq!(normalize_sha("5869D3D").as_deref(), Some("5869d3d"));
    }

    #[test]
    fn normalize_rejects_non_sha_output() {
        // 这些都不是版本号，混进记录只会误导归因，必须返回 None。
        assert_eq!(normalize_sha(""), None);
        assert_eq!(normalize_sha("fatal: not a git repository"), None);
        assert_eq!(normalize_sha("5869d3d-dirty"), None);
        assert_eq!(normalize_sha("abc"), None, "短于 7 位太容易撞库");
    }

    #[test]
    fn engine_revision_at_missing_dir_is_none() {
        assert_eq!(
            engine_revision_at(Path::new("/nonexistent/sisyphus/engine/dir")),
            None,
            "引擎源码目录不存在时必须如实返回未知，不得猜版本"
        );
    }

    #[test]
    fn engine_revision_at_non_repo_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            engine_revision_at(dir.path()),
            None,
            "非 git 目录应返回未知"
        );
    }

    #[test]
    fn dirty_revision_is_labelled() {
        // 标签必须区分「脏工作区」：同一个 commit 的脏样本不是那个版本的代码
        // 跑出来的，混为一谈会让指标看起来可归因、实际归因错了。
        let clean = EngineRevision {
            commit: "5869d3dbae51".to_string(),
            dirty: false,
        };
        let dirty = EngineRevision {
            commit: "5869d3dbae51".to_string(),
            dirty: true,
        };
        assert_eq!(clean.label(), "5869d3dbae51");
        assert_eq!(dirty.label(), "5869d3dbae51+dirty");
        assert_ne!(clean.label(), dirty.label());
    }

    #[test]
    fn engine_revision_resolves_in_this_repo() {
        // 单仓库开发场景：从编译时的源码目录能取到版本。
        // 分发的二进制（源码目录已不存在）会返回 None —— 那不是缺陷，
        // 所以这里只在「目录存在且是 git 仓库」时断言。
        let repo = Path::new(ENGINE_SOURCE_DIR);
        if !repo.join(".git").exists() && engine_revision().is_none() {
            return;
        }
        let rev = engine_revision().expect("仓库存在时应能取到引擎版本");
        assert_eq!(rev.commit.len(), 12, "默认取 12 位短 sha: {rev:?}");
        assert!(rev.commit.chars().all(|c| c.is_ascii_hexdigit()), "{rev:?}");
    }
}
