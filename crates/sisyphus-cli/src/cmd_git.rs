use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand)]
/// Git Commands.
pub enum GitCommands {
    /// 显示工作树状态
    Status {
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 显示工作树或已暂存变更的差异
    Diff {
        ///可选 pathspec 过滤条件
        pathspec: Option<String>,
        ///显示已暂存（缓存）的差异
        #[arg(short = 'c', long = "cached")]
        cached: bool,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 暂存文件
    Add {
        ///要暂存的路径（使用 "." 表示全部）
        pathspecs: Vec<String>,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 创建提交
    Commit {
        ///提交信息
        #[arg(short = 'm', long = "message")]
        message: String,
        ///允许空提交
        #[arg(long = "allow-empty")]
        allow_empty: bool,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 显示提交日志
    Log {
        ///要显示的提交数量
        #[arg(short = 'n', default_value = "10")]
        count: usize,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 列出分支
    Branch {
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 切换到分支（或使用 -b 创建并切换）
    Checkout {
        ///分支名
        branch: String,
        ///创建分支并切换过去
        #[arg(short = 'b')]
        create: bool,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 推送到远程
    Push {
        ///远程名（默认：origin）
        #[arg(default_value = "origin")]
        remote: String,
        ///要推送的分支（默认：当前分支）
        branch: Option<String>,
        ///强制推送
        #[arg(short = 'f', long = "force")]
        force: bool,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 从远程拉取
    Pull {
        ///远程名（默认：origin）
        #[arg(default_value = "origin")]
        remote: String,
        ///要拉取的分支（默认：当前分支）
        branch: Option<String>,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 显示单行摘要（分支 + 计数）
    Summary {
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
/// Build Commands.
pub enum BuildCommands {
    /// 检查代码错误
    Check {
        ///要检查的包
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        ///检查全部工作区成员
        #[arg(long = "all")]
        all: bool,
        ///语言后端（rust/node/python/go/java/gradle/fe）。省略时自动检测。
        #[arg(long = "lang")]
        lang: Option<String>,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 构建项目
    Build {
        ///要构建的包
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        ///以 release 模式构建
        #[arg(short = 'r', long = "release")]
        release: bool,
        ///语言后端（rust/node/python/go/java/gradle/fe）。省略时自动检测。
        #[arg(long = "lang")]
        lang: Option<String>,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 运行测试
    Test {
        ///仅运行名称匹配 `<filter>` 的测试
        #[arg(short = 'n', long = "name")]
        name: Option<String>,
        ///要测试的包
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        ///测试全部工作区成员
        #[arg(long = "all")]
        all: bool,
        ///语言后端（rust/node/python/go/java/gradle/fe）。省略时自动检测。
        #[arg(long = "lang")]
        lang: Option<String>,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 检查代码
    Lint {
        ///要检查的包
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        ///语言后端（rust/node/python/go/java/gradle/fe）。省略时自动检测。
        #[arg(long = "lang")]
        lang: Option<String>,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 格式化代码
    Fmt {
        ///检查格式但不修改文件
        #[arg(long = "check")]
        check: bool,
        ///语言后端（rust/node/python/go/java/gradle/fe）。省略时自动检测。
        #[arg(long = "lang")]
        lang: Option<String>,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
    /// 生成文档
    Doc {
        ///要生成文档的包
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        ///语言后端（rust/node/python/go/java/gradle/fe）。省略时自动检测。
        #[arg(long = "lang")]
        lang: Option<String>,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
/// Run Commands.
pub enum RunCommands {
    /// 运行项目（cargo run、npm start、go run . 等）
    Run {
        ///运行命令的额外参数（在 -- 之后传递）
        #[arg(last = true)]
        args: Vec<String>,
        ///语言后端（rust/node/python/go/java/gradle/fe）。省略时自动检测。
        #[arg(long = "lang")]
        lang: Option<String>,
        ///工作目录（默认：当前目录）
        #[arg(short = 'C')]
        cwd: Option<PathBuf>,
    },
}
