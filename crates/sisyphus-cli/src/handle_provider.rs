use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use anyhow::Result;
use sisyphus_types::{Config, ProviderConfig, ProviderKind};

use crate::commands::ProviderCommands;

/// 默认连通性测试提示词：短、确定、低 token。
const DEFAULT_TEST_PROMPT: &str = "Say hello in one sentence";

pub(crate) async fn handle_provider(
    sub: &ProviderCommands,
    config: &Config,
    config_path: Option<&Path>,
) -> Result<()> {
    match sub {
        ProviderCommands::List => {
            list(config);
            Ok(())
        },
        ProviderCommands::Test {
            name,
            prompt,
            timeout_secs,
        } => test(config, name.as_deref(), prompt, *timeout_secs).await,
        ProviderCommands::Fallback => {
            fallback(config);
            Ok(())
        },
        ProviderCommands::Login {
            name,
            kind,
            model,
            endpoint,
            api_key,
            yes,
            dry_run,
            skip_test,
            timeout_secs,
        } => {
            login(
                config,
                config_path,
                LoginInput {
                    name: name.clone(),
                    kind: kind.clone(),
                    model: model.clone(),
                    endpoint: endpoint.clone(),
                    api_key: api_key.clone(),
                    assume_yes: *yes,
                    dry_run: *dry_run,
                    skip_test: *skip_test,
                    timeout_secs: *timeout_secs,
                },
            )
            .await
        },
        ProviderCommands::Remove { name } => remove(config_path, name),
    }
}

fn list(config: &Config) {
    let providers = &config.llm.providers;
    if providers.is_empty() {
        println!("No providers configured.");
        println!(
            "  Using model '{}' at {}",
            config.llm.model, config.llm.endpoint,
        );
        println!("  Add one with: sisyphus provider login");
        return;
    }
    println!("Registered providers (fallback order):");
    for (i, p) in providers.iter().enumerate() {
        let key_status = if p.api_key.trim().is_empty() {
            "key: <missing>"
        } else if p.api_key.starts_with("env:") {
            "key: <env>"
        } else {
            "key: set"
        };
        println!(
            "  {}. {} (kind: {}, model: {}, endpoint: {}, {}, {})",
            i + 1,
            p.name,
            p.kind,
            p.model,
            p.endpoint,
            key_status,
            if p.enabled { "enabled" } else { "disabled" },
        );
    }
}

fn fallback(config: &Config) {
    let providers = &config.llm.providers;
    println!("Fallback chain for provider routing:");
    println!(
        "  1. Try registered providers in order ({} total, by priority)",
        providers.len()
    );
    for (i, p) in providers.iter().enumerate() {
        println!(
            "     {}. {} (kind: {}, model: {})",
            i + 1,
            p.name,
            p.kind,
            p.model
        );
    }
    println!(
        "  2. Fallback to default endpoint: {} with model '{}'",
        config.llm.endpoint, config.llm.model
    );
    println!();
    println!("Error handling in fallback:");
    println!("  - Transient/RateLimited errors -> skip to next provider");
    println!("  - Permanent/Auth errors -> abort immediately");
    println!("  - All providers exhausted -> fallback to default endpoint");
}

/// 连通性探测目标。
enum Probe {
    /// 只测这一个 provider：走 `ProviderRegistry` 的**单次**调用。
    ///
    /// 刻意不走 `chat_with_fallback` —— 诊断语义是「现在能不能通一次」，
    /// 而运行时的重试链会对不可达端点做 3 次指数退避重试（≈7s 纯等待），
    /// 让 `provider test` 在最需要快速反馈的场景里反而最慢。
    Provider(ProviderConfig),
    /// 测配置里的默认端点（`[llm] endpoint/model`）。
    Default(Box<Config>),
}

/// 执行一次**真实**连通性调用。
///
/// 返回 `Ok((模型名, 耗时, 响应文本))`；失败时返回原始错误文本（由调用方分类解释）。
async fn run_connectivity_test(
    probe: Probe,
    prompt: &str,
    timeout_secs: Option<u64>,
) -> std::result::Result<(String, std::time::Duration, String), String> {
    let model = match &probe {
        Probe::Provider(p) => p.model.clone(),
        Probe::Default(cfg) => cfg.llm.model.clone(),
    };
    let timeout = std::time::Duration::from_secs(timeout_secs.unwrap_or(120).max(1));

    // 外层总超时对两条路径都生效：registry 走 SharedHttpClient（默认 5 分钟），
    // 默认端点路径读 `request_timeout_secs`，都不能让一次诊断无限期挂着。
    let started = Instant::now();
    let call = async move {
        match probe {
            Probe::Provider(p) => {
                let registry =
                    sisyphus_tools_llm::build_provider_registry(std::slice::from_ref(&p))
                        .map_err(|e| format!("构造 provider 失败：{e}"))?;
                let provider = registry.get(&p.name).ok_or_else(|| {
                    format!("provider '{}' 未生效（enabled=false 或配置无效）", p.name)
                })?;
                let messages = vec![sisyphus_tools_llm::ChatMessage {
                    role: sisyphus_tools_llm::ChatRole::User,
                    content: prompt.to_string(),
                }];
                provider
                    .chat("", &messages, false)
                    .await
                    .map(|r| r.content)
                    .map_err(|e| e.to_string())
            },
            Probe::Default(cfg) => {
                let mut cfg = *cfg;
                if let Some(t) = timeout_secs {
                    cfg.llm.request_timeout_secs = t.max(1);
                }
                let client = sisyphus_tools_llm::DynamicLlmClient::new(Arc::new(RwLock::new(cfg)));
                sisyphus_tools_llm::LlmClient::chat(&client, "", prompt, false)
                    .await
                    .map_err(|e| e.to_string())
            },
        }
    };

    match tokio::time::timeout(timeout, call).await {
        Ok(Ok(content)) => Ok((model, started.elapsed(), content)),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(format!(
            "LLM request timed out after {}s for {model}",
            timeout.as_secs()
        )),
    }
}

/// 把常见的失败原因翻译成可执行建议。
///
/// 只做**字符串分类**（错误跨 crate 边界已是 `String`/`TypesError`），但覆盖
/// 用户实际会遇到的绝大多数分支；未命中时返回 `None`，调用方仍打印原始错误。
fn explain_failure(err: &str) -> Option<&'static str> {
    let has = |s: &str| err.contains(s);

    if has("timed out") {
        return Some(
            "请求超时：本地模型或慢速网关需要更长的超时 —— 用 `provider test --timeout-secs 120`，\
             或在配置里调大 `[llm] request_timeout_secs`（默认 5s）",
        );
    }
    if has("401") {
        return Some(
            "密钥无效或已过期（401）：用 `provider login` 重新录入，或用 `--api-key env:VAR` 引用环境变量",
        );
    }
    if has("403") {
        return Some("无权访问该模型/区域受限（403）：确认模型名与账号权限、以及是否需要代理");
    }
    if has("404") {
        return Some(
            "端点或模型不存在（404）：OpenAI 兼容端点通常以 `/v1/chat/completions` 结尾；\
             模型名需与 provider 侧完全一致",
        );
    }
    if has("429") {
        return Some("被限流或额度不足（429）：稍后重试，或换用其它 provider");
    }
    if has("Failed to parse response") {
        return Some(
            "响应不是 OpenAI 兼容 JSON：多半是 kind 选错（google/anthropic/cohere 用各自协议），\
             或端点指向了网页而非 API",
        );
    }
    if has("missing content field") {
        return Some("响应缺少 `choices[0].message.content`：确认这是 chat completions 端点");
    }
    if has("connection refused") || has("error sending request") || has("dns error") {
        return Some("无法建立连接：确认服务已启动、地址/端口正确、网络可达（可先 curl 该端点）");
    }
    None
}

async fn test(
    config: &Config,
    name: Option<&str>,
    prompt: &str,
    timeout_secs: Option<u64>,
) -> Result<()> {
    let prompt = if prompt.trim().is_empty() {
        DEFAULT_TEST_PROMPT
    } else {
        prompt
    };

    // 指定名称时只测该 provider（单次尝试）；未指定时测默认端点。
    let (probe, label, kind, model, endpoint) = match name {
        Some(name) => {
            let mut p = config
                .llm
                .providers
                .iter()
                .find(|p| p.name == name)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Provider '{name}' not found. Use `provider list` to see registered providers."
                    )
                })?;
            p.enabled = true;
            let label = p.name.clone();
            let kind = p.kind.to_string();
            let model = p.model.clone();
            let endpoint = p.endpoint.clone();
            (Probe::Provider(p), label, kind, model, endpoint)
        },
        None => {
            let mut cfg = config.clone();
            // 只测默认端点：清空 providers，避免 registry 抢先接管
            cfg.llm.providers.clear();
            let kind = "openai-compatible".to_string();
            let model = cfg.llm.model.clone();
            let endpoint = cfg.llm.endpoint.clone();
            (
                Probe::Default(Box::new(cfg)),
                "<default endpoint>".to_string(),
                kind,
                model,
                endpoint,
            )
        },
    };
    println!("Testing provider '{label}' (kind: {kind}, model: {model})");
    println!("  endpoint: {endpoint}");
    println!("  prompt: {prompt}");

    match run_connectivity_test(probe, prompt, timeout_secs).await {
        Ok((model, elapsed, text)) => {
            println!("  ✅ PASS  model={model} latency={}ms", elapsed.as_millis());
            let preview: String = text.trim().chars().take(200).collect();
            println!("  response: {preview}");
            Ok(())
        },
        Err(err) => {
            println!("  ❌ FAIL  {err}");
            if let Some(hint) = explain_failure(&err) {
                println!("  hint: {hint}");
            }
            anyhow::bail!("provider connectivity test failed")
        },
    }
}

/// `provider login` 的输入（命令行 flag 或交互补齐）。
struct LoginInput {
    name: Option<String>,
    kind: Option<String>,
    model: Option<String>,
    endpoint: Option<String>,
    api_key: Option<String>,
    assume_yes: bool,
    dry_run: bool,
    skip_test: bool,
    timeout_secs: Option<u64>,
}

fn prompt_line(label: &str, default: Option<&str>) -> Result<String> {
    match default {
        Some(d) => print!("{label} [{d}]: "),
        None => print!("{label}: "),
    }
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let value = line.trim().to_string();
    Ok(if value.is_empty() {
        default.unwrap_or_default().to_string()
    } else {
        value
    })
}

fn supported_kinds() -> [ProviderKind; 8] {
    [
        ProviderKind::OpenAi,
        ProviderKind::Anthropic,
        ProviderKind::Google,
        ProviderKind::Groq,
        ProviderKind::Mistral,
        ProviderKind::Together,
        ProviderKind::OpenRouter,
        ProviderKind::Cohere,
    ]
}

/// 交互式引导：选择 → 输入 → 连通性测试 → 写配置。
#[allow(clippy::too_many_lines)]
async fn login(_config: &Config, config_path: Option<&Path>, mut input: LoginInput) -> Result<()> {
    let interactive = std::io::stdin().is_terminal() && !input.assume_yes;

    // 1) kind 选择
    let kind = match input.kind.take() {
        Some(raw) => raw.parse::<ProviderKind>().map_err(|e| {
            anyhow::anyhow!(
                "{e}（可选：openai/anthropic/google/groq/mistral/together/openrouter/cohere）"
            )
        })?,
        None if interactive => {
            let kinds = supported_kinds();
            println!("选择 provider 类型：");
            for (i, k) in kinds.iter().enumerate() {
                let default_model = k.default_model();
                println!("  {}. {k} (默认模型 {default_model})", i + 1);
            }
            let raw = prompt_line("类型序号或名称", Some("1"))?;
            match raw.parse::<usize>() {
                Ok(n) if (1..=kinds.len()).contains(&n) => kinds[n - 1],
                _ => raw
                    .parse::<ProviderKind>()
                    .map_err(|e| anyhow::anyhow!("{e}（输入序号 1-{} 或名称）", kinds.len()))?,
            }
        },
        None => ProviderKind::OpenAi,
    };

    // 2) 基础字段
    let default_model = kind.default_model().to_string();
    let default_endpoint = kind.default_endpoint();

    let name = match input.name.take() {
        Some(n) if !n.trim().is_empty() => n,
        _ if interactive => {
            let suggested = format!("{kind}-{}", default_model.replace(['.', '/', ':'], "-"));
            prompt_line(
                "提供商标识（用于之后 `provider test <name>`）",
                Some(&suggested),
            )?
        },
        _ => anyhow::bail!(
            "缺少 --name（非交互模式）。示例：provider login --name ollama-local --kind openai \
             --endpoint http://127.0.0.1:11434/v1/chat/completions --model qwen2.5-coder:3b \
             --api-key ollama --yes"
        ),
    };

    let model = match input.model.take() {
        Some(m) if !m.trim().is_empty() => m,
        _ if interactive => prompt_line("模型名", Some(&default_model))?,
        _ => default_model.clone(),
    };

    let endpoint = match input.endpoint.take() {
        Some(e) if !e.trim().is_empty() => e,
        _ if interactive => prompt_line(
            "API 端点（OpenAI 兼容需以 /v1/chat/completions 结尾）",
            default_endpoint,
        )?,
        _ => default_endpoint.map(str::to_string).unwrap_or_default(),
    };
    if endpoint.trim().is_empty() {
        anyhow::bail!(
            "缺少 --endpoint（{kind} 没有通用默认端点）。示例：--endpoint https://…/v1/chat/completions"
        );
    }

    // 3) API key（支持 `env:VAR` 引用，避免明文落盘）
    let api_key = match input.api_key.take() {
        Some(k) if !k.trim().is_empty() => k,
        _ if interactive => {
            println!("API key（推荐输入 env:VAR 引用环境变量，避免明文写入配置）");
            prompt_line("API key", Some("env:PROVIDER_API_KEY"))?
        },
        _ => {
            anyhow::bail!("缺少 --api-key（非交互模式）。可传 `--api-key env:MY_KEY` 引用环境变量")
        },
    };

    let provider = ProviderConfig {
        name: name.clone(),
        kind,
        endpoint: endpoint.clone(),
        api_key: api_key.clone(),
        model: model.clone(),
        base_url: None,
        priority: 10,
        enabled: true,
        max_tokens: None,
        temperature: None,
    };

    // 4) 连通性测试（写配置**之前**做，避免把不可用的 provider 落盘）
    if input.skip_test {
        println!("跳过连通性测试（--skip-test）");
    } else {
        println!("连通性测试中…（prompt: {DEFAULT_TEST_PROMPT}）");
        match run_connectivity_test(
            Probe::Provider(provider.clone()),
            DEFAULT_TEST_PROMPT,
            input.timeout_secs,
        )
        .await
        {
            Ok((_, elapsed, text)) => {
                println!("  ✅ PASS  latency={}ms", elapsed.as_millis());
                println!(
                    "  response: {}",
                    text.trim().chars().take(200).collect::<String>()
                );
            },
            Err(err) => {
                println!("  ❌ FAIL  {err}");
                if let Some(hint) = explain_failure(&err) {
                    println!("  hint: {hint}");
                }
                anyhow::bail!(
                    "连通性测试失败，未写入配置。修好后重试，或确认可用时加 `--skip-test` 强制写入"
                );
            },
        }
    }

    if input.dry_run {
        println!("--dry-run：不写入配置。将写入的 provider：");
        println!(
            "  name={} kind={} model={} endpoint={} api_key={}",
            provider.name,
            provider.kind,
            provider.model,
            provider.endpoint,
            if provider.api_key.starts_with("env:") {
                "<env>"
            } else {
                "<inline>"
            },
        );
        return Ok(());
    }

    // 5) 写配置（upsert + 备份）
    let path = resolve_config_path(config_path);
    let mut cfg = load_for_write(&path)?;
    let replaced = upsert_provider(&mut cfg, provider.clone());
    backup_config(&path)?;
    cfg.save_to_file(&path.to_string_lossy())?;
    harden_permissions_if_inline_key(&path, &provider);

    println!(
        "{} provider '{}' -> {}",
        if replaced { "Updated" } else { "Registered" },
        provider.name,
        path.display()
    );
    println!("Next: `sisyphus provider test {}`", provider.name);
    Ok(())
}

fn remove(config_path: Option<&Path>, name: &str) -> Result<()> {
    let path = resolve_config_path(config_path);
    if !path.exists() {
        anyhow::bail!("配置文件不存在：{}", path.display());
    }
    let mut cfg = load_for_write(&path)?;
    let before = cfg.llm.providers.len();
    cfg.llm.providers.retain(|p| p.name != name);
    if cfg.llm.providers.len() == before {
        anyhow::bail!(
            "未找到 provider '{name}'（现有：{}）",
            cfg.llm.providers.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
        );
    }
    backup_config(&path)?;
    cfg.save_to_file(&path.to_string_lossy())?;
    println!("Removed provider '{name}' from {}", path.display());
    Ok(())
}

/// 解析要写入的配置文件路径。
///
/// 优先级：显式 `--config` > 当前目录已有配置文件 > `sisyphus.toml`。
fn resolve_config_path(explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return p.to_path_buf();
    }
    ["sisyphus.toml", "config.toml", "sisyphus.local.toml"]
        .iter()
        .find(|p| Path::new(p).exists())
        .map_or_else(|| PathBuf::from("sisyphus.toml"), PathBuf::from)
}

fn load_for_write(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    Config::from_file(&path.to_string_lossy())
        .map_err(|e| anyhow::anyhow!("读取配置失败 {}: {e}", path.display()))
}

/// 写入前备份原文件（`<path>.bak`）。
///
/// `Config::save_to_file` 会整体重写 TOML，**注释与未知字段会丢失**；备份是
/// 用户找回它们的唯一途径，因此只要有原文件就必须留一份。
fn backup_config(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let backup = path.with_extension(format!(
        "{}bak",
        path.extension().and_then(|e| e.to_str()).map_or(String::new(), |e| format!("{e}."))
    ));
    std::fs::copy(path, &backup)
        .map_err(|e| anyhow::anyhow!("备份配置失败 {}: {e}", backup.display()))?;
    // 备份可能含明文密钥（上一轮写入的 provider），必须与主文件同权限，
    // 否则 `cp`/`save` 保留的 0644 会把密钥暴露给同机其他用户。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&backup) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(&backup, perms);
        }
    }
    println!("Backup: {}", backup.display());
    Ok(())
}

/// 含明文密钥的配置文件收紧到 0600（仅 Unix）。
fn harden_permissions_if_inline_key(path: &Path, provider: &ProviderConfig) {
    if provider.api_key.starts_with("env:") {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            if std::fs::set_permissions(path, perms).is_ok() {
                println!("Permissions: 0600（配置内含明文密钥）");
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// upsert provider（同名覆盖）。返回是否覆盖了已有项。
fn upsert_provider(cfg: &mut Config, provider: ProviderConfig) -> bool {
    if let Some(existing) = cfg.llm.providers.iter_mut().find(|p| p.name == provider.name) {
        *existing = provider;
        true
    } else {
        cfg.llm.providers.push(provider);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(name: &str, model: &str) -> ProviderConfig {
        ProviderConfig {
            name: name.to_string(),
            kind: ProviderKind::OpenAi,
            endpoint: "http://127.0.0.1:1/v1/chat/completions".to_string(),
            api_key: "env:TEST_KEY".to_string(),
            model: model.to_string(),
            base_url: None,
            priority: 10,
            enabled: true,
            max_tokens: None,
            temperature: None,
        }
    }

    #[test]
    fn upsert_appends_then_replaces_by_name() {
        let mut cfg = Config::default();
        assert!(!upsert_provider(&mut cfg, provider("a", "m1")));
        assert_eq!(cfg.llm.providers.len(), 1);

        // 同名 → 覆盖，不新增
        assert!(upsert_provider(&mut cfg, provider("a", "m2")));
        assert_eq!(cfg.llm.providers.len(), 1);
        assert_eq!(cfg.llm.providers[0].model, "m2");

        // 不同名 → 追加
        assert!(!upsert_provider(&mut cfg, provider("b", "m3")));
        assert_eq!(cfg.llm.providers.len(), 2);
    }

    #[test]
    fn failure_classification_is_actionable() {
        // 超时必须给出「调大超时」的可执行建议，而不是泛泛的失败
        let hint = explain_failure("LLM request timed out after 5s for gpt-4o").expect("应有建议");
        assert!(hint.contains("request_timeout_secs"), "hint={hint}");

        assert!(explain_failure("LLM API error (401): invalid key")
            .expect("401 应有建议")
            .contains("provider login"));
        assert!(explain_failure("LLM API error (404): model not found")
            .expect("404 应有建议")
            .contains("v1/chat/completions"));
        assert!(
            explain_failure("Failed to parse response JSON: expected value")
                .expect("解析失败应有建议")
                .contains("kind")
        );
        // provider registry 路径的措辞（"Failed to parse response: ..."）同样要命中，
        // 否则协议选错时用户只看到一句 error decoding response body。
        assert!(explain_failure(
            "Transient: Failed to parse response: error decoding response body"
        )
        .expect("registry 路径解析失败应有建议")
        .contains("kind"));

        // 未命中分类时保持沉默（调用方仍打印原始错误）
        assert!(explain_failure("something entirely unexpected").is_none());
    }

    #[test]
    fn remove_is_pure_on_config_vec() {
        let mut cfg = Config::default();
        upsert_provider(&mut cfg, provider("a", "m1"));
        upsert_provider(&mut cfg, provider("b", "m2"));
        cfg.llm.providers.retain(|p| p.name != "a");
        assert_eq!(cfg.llm.providers.len(), 1);
        assert_eq!(cfg.llm.providers[0].name, "b");
    }

    #[test]
    fn kind_names_round_trip_for_login() {
        for kind in supported_kinds() {
            let parsed: ProviderKind = kind.to_string().parse().expect("kind 名称应可回环");
            assert_eq!(parsed, kind);
        }
    }
}
