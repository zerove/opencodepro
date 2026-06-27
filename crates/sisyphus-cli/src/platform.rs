use std::env;
use std::path::MAIN_SEPARATOR;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformInfo {
    pub os: String,
    pub shell: String,
    pub path_sep: char,
    pub is_windows: bool,
}

pub fn detect_platform() -> PlatformInfo {
    PlatformInfo {
        os: os_name(),
        shell: default_shell(),
        path_sep: MAIN_SEPARATOR,
        is_windows: cfg!(windows),
    }
}

fn os_name() -> String {
    if cfg!(windows) {
        "windows".to_string()
    } else if cfg!(target_os = "macos") {
        "macos".to_string()
    } else {
        "unix".to_string()
    }
}

#[cfg(unix)]
mod unix {
    pub fn default_shell() -> String {
        "/bin/bash".to_string()
    }
    #[allow(dead_code)]
    pub fn path_env_separator() -> char {
        ':'
    }
}

#[cfg(windows)]
mod windows {
    pub fn default_shell() -> String {
        "powershell.exe".to_string()
    }
    #[allow(dead_code)]
    pub fn path_env_separator() -> char {
        ';'
    }
}

fn default_shell() -> String {
    #[cfg(unix)]
    {
        unix::default_shell()
    }
    #[cfg(windows)]
    {
        windows::default_shell()
    }
}

#[allow(dead_code)]
pub fn normalize_path(path: &str) -> String {
    if cfg!(windows) {
        path.replace("/", "\\")
    } else {
        path.replace("\\", "/")
    }
}

#[allow(dead_code)]
pub fn home_dir() -> Option<String> {
    if cfg!(windows) {
        env::var("USERPROFILE").ok()
    } else {
        env::var("HOME").ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_platform() {
        let info = detect_platform();
        if cfg!(windows) {
            assert!(info.is_windows);
            assert_eq!(info.os, "windows");
            assert_eq!(info.shell, "powershell.exe");
            assert_eq!(info.path_sep, '\\');
        } else {
            assert!(!info.is_windows);
            assert_eq!(info.path_sep, '/');
        }
    }

    #[test]
    fn test_normalize_path() {
        if cfg!(windows) {
            assert_eq!(normalize_path("foo/bar"), "foo\\bar");
        } else {
            assert_eq!(normalize_path("foo\\bar"), "foo/bar");
        }
    }

    #[test]
    fn test_home_dir() {
        let home = home_dir();
        // We can't easily assert the value, but it should be Some in most environments
        // unless running in a very restricted CI.
        if let Some(h) = home {
            assert!(!h.is_empty());
        }
    }
}
