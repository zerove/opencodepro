//! CLI 权限规则与审批审计。
//!
//! ## 规则文件
//!
//! 默认从 `./.sisyphus-permissions.json` 加载，格式为数组：
//!
//! ```json
//! [
//!   { "pattern": "exec", "permission": "ask" },
//!   { "pattern": "builtin:*", "permission": "allow" },
//!   { "pattern": "jcode/*", "permission": "deny" }
//! ]
//! ```
//!
//! `pattern` 使用 glob 语法（`*`、`?`、`[...]`），按**声明顺序**匹配，首条命中生效。
//! 语法非法的 pattern 退化为精确字符串比较，不会静默放行。
//!
//! ## 默认判定
//!
//! - 未命中任何规则且工具属于 [`DANGEROUS_TOOLS`] → `ask`（需要人工确认）。
//! - 其余情况 → `allow`（保持既有行为的向后兼容）。
//!
//! ## 审计
//!
//! 每次判定（含交互式确认结果）以 JSONL 追加到
//! `<data-dir>/audit/permissions.jsonl`；可用 `SISYPHUS_PERMISSION_AUDIT`
//! 指定文件，或用 `sisyphus permission audit` 查看。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 未显式配置时默认需要人工确认的工具。
pub const DANGEROUS_TOOLS: &[&str] = &["exec", "bash", "bash_exec", "builtin:bash_exec", "shell"];

/// 权限判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// 允许执行。
    Allow,
    /// 拒绝执行。
    Deny,
    /// 需要人工确认。
    Ask,
}

impl Decision {
    /// 用于审计与展示的稳定字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Ask => "ask",
        }
    }

    /// 转换为 `Option<bool>` 形式（`Ask` → `None`），兼容既有调用方。
    pub fn as_optional_bool(self) -> Option<bool> {
        match self {
            Self::Allow => Some(true),
            Self::Deny => Some(false),
            Self::Ask => None,
        }
    }
}

/// 判定来源，写入审计便于解释"为什么是这个结果"。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionSource {
    /// 命中规则，附带原始 pattern。
    Rule(String),
    /// 危险工具默认询问。
    DangerousDefault,
    /// 默认允许。
    Default,
}

impl DecisionSource {
    /// 审计中的字符串形式。
    pub fn as_audit_str(&self) -> String {
        match self {
            Self::Rule(pattern) => format!("rule:{pattern}"),
            Self::DangerousDefault => "dangerous-default".to_string(),
            Self::Default => "default".to_string(),
        }
    }
}

/// 一次判定的完整结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDecision {
    /// 判定结果。
    pub decision: Decision,
    /// 判定来源。
    pub source: DecisionSource,
}

impl PermissionDecision {
    /// 兼容既有 `check()` 语义。
    pub fn as_optional_bool(&self) -> Option<bool> {
        self.decision.as_optional_bool()
    }
}

/// 一条权限审计记录。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionAuditEntry {
    /// RFC3339 时间戳（UTC）。
    pub ts: String,
    /// 被检查的工具 ID。
    pub tool: String,
    /// 规则判定：`allow` / `deny` / `ask`。
    pub decision: String,
    /// 判定来源：`rule:<pattern>` / `dangerous-default` / `default`。
    pub source: String,
    /// 最终结果：`allowed` / `denied` / `asked-approved` / `asked-denied` / `asked-unanswered`。
    pub resolved: String,
    /// 执行时的工作目录。
    pub cwd: String,
    /// 当前用户（读取 `USER`）。
    pub user: Option<String>,
}

/// 从配置文件加载规则的 CLI 权限存储。
#[derive(Debug, Default, Clone)]
pub struct CliPermission {
    rules: Vec<Rule>,
    audit_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct Rule {
    pattern: String,
    permission: crate::tools::Permission,
    glob: Option<glob::Pattern>,
}

impl CliPermission {
    /// 从 JSON 配置文件加载权限规则。
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let config_path = path.unwrap_or_else(default_config_path);

        let mut this = Self {
            rules: Vec::new(),
            audit_path: None,
        };

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .context("Failed to read permission config")?;
            let raw: Vec<RawRule> =
                serde_json::from_str(&content).context("Failed to parse permission config")?;
            for r in raw {
                let permission = match r.permission.to_lowercase().as_str() {
                    "allow" => crate::tools::Permission::Allow,
                    "deny" => crate::tools::Permission::Deny,
                    "ask" => crate::tools::Permission::Ask,
                    other => anyhow::bail!("Unknown permission value: {other}"),
                };
                let glob = glob::Pattern::new(&r.pattern).ok();
                this.rules.push(Rule {
                    pattern: r.pattern,
                    permission,
                    glob,
                });
            }
        }

        Ok(this)
    }

    /// 指定审计日志文件路径（默认由环境变量或 `.sisyphus/audit` 推导）。
    pub fn with_audit_path(mut self, path: PathBuf) -> Self {
        self.audit_path = Some(path);
        self
    }

    /// 当前审计日志路径。
    pub fn audit_path(&self) -> PathBuf {
        self.audit_path.clone().unwrap_or_else(default_audit_path)
    }

    /// 规则数量（便于诊断输出）。
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// 完整判定：规则 → 危险默认 → 默认允许。
    pub fn evaluate(&self, tool_name: &str) -> PermissionDecision {
        for rule in &self.rules {
            let matched = match &rule.glob {
                Some(pattern) => pattern.matches(tool_name),
                // 非法 glob 退化为精确比较，避免"写错的规则等于放行"
                None => rule.pattern == tool_name,
            };
            if matched {
                return PermissionDecision {
                    decision: match rule.permission {
                        crate::tools::Permission::Allow => Decision::Allow,
                        crate::tools::Permission::Deny => Decision::Deny,
                        crate::tools::Permission::Ask => Decision::Ask,
                    },
                    source: DecisionSource::Rule(rule.pattern.clone()),
                };
            }
        }

        if is_dangerous(tool_name) {
            return PermissionDecision {
                decision: Decision::Ask,
                source: DecisionSource::DangerousDefault,
            };
        }

        PermissionDecision {
            decision: Decision::Allow,
            source: DecisionSource::Default,
        }
    }

    /// 检查工具是否被允许。允许时返回 `Some(true)`，拒绝时返回 `Some(false)`，
    /// 需要询问时返回 `None`。
    pub fn check(&self, tool_name: &str) -> Option<bool> {
        self.evaluate(tool_name).as_optional_bool()
    }

    /// 记录一次判定结果（append JSONL）。写入失败只告警，不影响主流程。
    pub fn record_decision(&self, tool: &str, decision: &PermissionDecision, resolved: &str) {
        if let Err(err) = self.append_audit(tool, decision, resolved) {
            eprintln!(
                "warning: 权限审计写入失败（{}）: {err}",
                self.audit_path().display()
            );
        }
    }

    fn append_audit(
        &self,
        tool: &str,
        decision: &PermissionDecision,
        resolved: &str,
    ) -> Result<()> {
        let path = self.audit_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建审计目录失败: {}", parent.display()))?;
        }
        let entry = PermissionAuditEntry {
            ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            tool: tool.to_string(),
            decision: decision.decision.as_str().to_string(),
            source: decision.source.as_audit_str(),
            resolved: resolved.to_string(),
            cwd: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "<unknown>".to_string()),
            user: std::env::var("USER").ok(),
        };
        let line = serde_json::to_string(&entry).context("序列化审计记录失败")?;
        use std::io::Write;
        // 行内容与换行**一次**写出：分两次 write 时，并发追加方可能插在
        // 中间（O_APPEND 只保证单次 write 的原子性），产出
        // `{"a":1}{"b":2}` 这种非法 JSONL 行，审计查询随之失效。
        let mut payload = line.into_bytes();
        payload.push(b'\n');
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("打开审计文件失败: {}", path.display()))?;
        file.write_all(&payload).context("写入审计记录失败")
    }
}

/// 交互式确认：判定 → 记录审计 → `Allow` 静默通过、`Deny` 直接失败、
/// `Ask` 在终端提示 `[Y/n]`（默认同意）。
///
/// CLI 侧所有“可能执行任意命令”的入口都应走这里，保证审计口径一致，
/// 而不是让不同入口各自复制一份提示逻辑。
pub fn require_tool(perms: &CliPermission, tool: &str) -> Result<()> {
    use std::io::Write;

    let decision = perms.evaluate(tool);
    match decision.decision {
        Decision::Allow => {
            perms.record_decision(tool, &decision, "allowed");
            Ok(())
        },
        Decision::Deny => {
            perms.record_decision(tool, &decision, "denied");
            if tool == "exec" {
                anyhow::bail!("Permission denied: shell execution is blocked by config");
            }
            anyhow::bail!("Permission denied: {tool} is blocked by config");
        },
        Decision::Ask => {
            let label = if tool == "exec" {
                "shell execution"
            } else {
                tool
            };
            eprint!("⚠  Allow {label}? [Y/n] ");
            let _ = std::io::stderr().flush();
            let mut buf = String::new();
            let read = std::io::stdin().read_line(&mut buf);
            if read.is_err() {
                perms.record_decision(tool, &decision, "asked-unanswered");
                anyhow::bail!("No interactive input available to approve {tool}");
            }
            let answer = buf.trim().to_lowercase();
            if !answer.is_empty() && answer != "y" && answer != "yes" {
                perms.record_decision(tool, &decision, "asked-denied");
                if tool == "exec" {
                    anyhow::bail!("Shell execution denied by user");
                }
                anyhow::bail!("{tool} denied by user");
            }
            perms.record_decision(tool, &decision, "asked-approved");
            Ok(())
        },
    }
}

/// 读取审计日志（最近 `limit` 条，按时间顺序返回）。
pub fn read_audit(path: &Path, limit: usize) -> Result<Vec<PermissionAuditEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("读取审计文件失败: {}", path.display()))?;
    let mut entries: Vec<PermissionAuditEntry> = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry: PermissionAuditEntry = serde_json::from_str(line)
            .with_context(|| format!("第 {} 行不是合法审计记录", idx + 1))?;
        entries.push(entry);
    }
    if entries.len() > limit {
        entries.drain(..entries.len() - limit);
    }
    Ok(entries)
}

fn is_dangerous(tool_name: &str) -> bool {
    DANGEROUS_TOOLS.contains(&tool_name)
}

fn default_config_path() -> PathBuf {
    PathBuf::from(".sisyphus-permissions.json")
}

fn default_audit_path() -> PathBuf {
    if let Some(path) = std::env::var_os("SISYPHUS_PERMISSION_AUDIT") {
        return PathBuf::from(path);
    }
    let base = std::env::var_os("SISYPHUS_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".sisyphus"));
    base.join("audit").join("permissions.jsonl")
}

#[derive(serde::Deserialize)]
struct RawRule {
    pattern: String,
    permission: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn rules(spec: &[(&str, &str)]) -> CliPermission {
        let mut perm = CliPermission::default();
        for (pattern, decision) in spec {
            let permission = match *decision {
                "allow" => crate::tools::Permission::Allow,
                "deny" => crate::tools::Permission::Deny,
                _ => crate::tools::Permission::Ask,
            };
            perm.rules.push(Rule {
                pattern: (*pattern).to_string(),
                permission,
                glob: glob::Pattern::new(pattern).ok(),
            });
        }
        perm
    }

    #[test]
    fn glob_patterns_match_tool_ids() {
        let perm = rules(&[("builtin:*", "deny"), ("file_*", "allow")]);
        assert_eq!(perm.check("builtin:file_read"), Some(false));
        assert_eq!(perm.check("file_write"), Some(true));
    }

    #[test]
    fn first_matching_rule_wins() {
        let perm = rules(&[("exec", "allow"), ("*", "deny")]);
        assert_eq!(perm.check("exec"), Some(true));
        assert_eq!(perm.check("other"), Some(false));
    }

    #[test]
    fn dangerous_tools_default_to_ask() {
        let perm = CliPermission::default();
        let decision = perm.evaluate("exec");
        assert_eq!(decision.decision, Decision::Ask);
        assert_eq!(decision.source, DecisionSource::DangerousDefault);
        assert_eq!(perm.check("exec"), None);
    }

    #[test]
    fn explicit_rule_overrides_dangerous_default() {
        let perm = rules(&[("exec", "allow")]);
        assert_eq!(perm.check("exec"), Some(true));
    }

    #[test]
    fn non_dangerous_defaults_to_allow() {
        let perm = CliPermission::default();
        let decision = perm.evaluate("file_read");
        assert_eq!(decision.decision, Decision::Allow);
        assert_eq!(decision.source, DecisionSource::Default);
    }

    #[test]
    fn invalid_glob_falls_back_to_exact_match() {
        let perm = rules(&[("bad[pattern", "deny")]);
        assert_eq!(perm.check("bad[pattern"), Some(false));
        assert_eq!(
            perm.check("badXpattern"),
            Some(true),
            "非法 glob 不得退化为放行"
        );
    }

    #[test]
    fn audit_entries_are_jsonl_and_readable() {
        let dir = TempDir::new().expect("tmpdir");
        let audit = dir.path().join("audit/permissions.jsonl");
        let perm = CliPermission::default().with_audit_path(audit.clone());

        let allowed = perm.evaluate("file_read");
        perm.record_decision("file_read", &allowed, "allowed");
        let asked = perm.evaluate("exec");
        perm.record_decision("exec", &asked, "asked-denied");

        let entries = read_audit(&audit, 10).expect("读取审计");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].tool, "file_read");
        assert_eq!(entries[0].decision, "allow");
        assert_eq!(entries[0].source, "default");
        assert_eq!(entries[0].resolved, "allowed");
        assert_eq!(entries[1].tool, "exec");
        assert_eq!(entries[1].decision, "ask");
        assert_eq!(entries[1].source, "dangerous-default");
        assert_eq!(entries[1].resolved, "asked-denied");
        assert!(!entries[0].ts.is_empty());

        // 行数等于记录数（每行一条，末尾换行）
        let raw = std::fs::read_to_string(&audit).expect("读文件");
        assert_eq!(raw.lines().count(), 2);
    }

    #[test]
    fn read_audit_returns_last_n_entries() {
        let dir = TempDir::new().expect("tmpdir");
        let audit = dir.path().join("permissions.jsonl");
        let perm = CliPermission::default().with_audit_path(audit.clone());
        for tool in ["a", "b", "c"] {
            let decision = perm.evaluate(tool);
            perm.record_decision(tool, &decision, "allowed");
        }
        let entries = read_audit(&audit, 2).expect("读取审计");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].tool, "b");
        assert_eq!(entries[1].tool, "c");
    }

    #[test]
    fn read_audit_missing_file_is_empty() {
        let dir = TempDir::new().expect("tmpdir");
        let entries = read_audit(&dir.path().join("nope.jsonl"), 5).expect("缺失文件不应报错");
        assert!(entries.is_empty());
    }

    #[test]
    fn require_tool_allows_and_audits() {
        let dir = TempDir::new().expect("tmpdir");
        let audit = dir.path().join("permissions.jsonl");
        let perm = CliPermission::default().with_audit_path(audit.clone());

        require_tool(&perm, "file_read").expect("默认允许");
        let entries = read_audit(&audit, 10).expect("读取审计");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].resolved, "allowed");
    }

    #[test]
    fn require_tool_denies_and_audits() {
        let dir = TempDir::new().expect("tmpdir");
        let audit = dir.path().join("permissions.jsonl");
        let perm = rules(&[("exec", "deny")]).with_audit_path(audit.clone());

        let err = require_tool(&perm, "exec").expect_err("规则拒绝");
        assert!(err.to_string().contains("Permission denied"), "err={err}");
        let entries = read_audit(&audit, 10).expect("读取审计");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].decision, "deny");
        assert_eq!(entries[0].resolved, "denied");
        assert_eq!(entries[0].source, "rule:exec");
    }

    #[test]
    fn load_rejects_unknown_permission_value() {
        let dir = TempDir::new().expect("tmpdir");
        let path = dir.path().join("perms.json");
        std::fs::write(&path, r#"[{"pattern":"exec","permission":"maybe"}]"#).expect("write");
        let err = CliPermission::load(Some(path)).expect_err("非法取值必须报错");
        assert!(err.to_string().contains("Unknown permission value"));
    }

    #[test]
    fn test_concurrent_audit_appends_keep_each_line_valid_json() {
        // 回归：审计行与换行分两次 write 时，并发追加可能把两行拼在一起，
        // 产出非法 JSONL（审计查询随之失效）。
        use std::sync::Arc;
        let dir = std::env::temp_dir().join(format!("sisyphus-perm-audit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建临时目录");
        let audit = dir.join("audit").join("permissions.jsonl");
        let perms = CliPermission::default().with_audit_path(audit.clone());
        let perms = Arc::new(perms);

        let mut handles = Vec::new();
        for i in 0..8 {
            let perms = perms.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..25 {
                    let decision = perms.evaluate("exec");
                    perms.record_decision("exec", &decision, "allowed");
                }
                let _ = i;
            }));
        }
        for handle in handles {
            handle.join().expect("写线程不应 panic");
        }

        let content = std::fs::read_to_string(&audit).expect("读取审计文件");
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 8 * 25, "每条记录都应独占一行");
        for line in lines {
            serde_json::from_str::<serde_json::Value>(line)
                .unwrap_or_else(|e| panic!("审计行不是合法 JSON（{e}）: {line}"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
