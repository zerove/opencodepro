use zed_extension_api::{self as zed, Result};

struct SisyphusExtension;

impl SisyphusExtension {
    fn new() -> Self {
        Self
    }
}

impl zed::Extension for SisyphusExtension {
    fn new() -> Self {
        Self::new()
    }

    fn label(&self) -> &str {
        "Sisyphus"
    }

    fn installation_message(&self) -> Option<zed::InstallationMessage> {
        Some(zed::InstallationMessage {
            heading: "Install Sisyphus Cognitive Engine".into(),
            message: "Sisyphus requires a running instance of the cognitive engine.\n\nPlease install and start sisyphus-bridge:\n\n  cargo install sisyphus-bridge\n  sisyphus-bridge serve".into(),
            check: Box::new(|| {
                // Check if sisyphus is available
                std::process::Command::new("sisyphus")
                    .arg("--version")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            }),
        })
    }
}

zed::register_extension!(SisyphusExtension);
