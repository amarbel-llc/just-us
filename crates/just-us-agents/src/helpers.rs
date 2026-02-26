use serde_json::Value;
use tokio::process::Command;

pub struct JustOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

pub async fn get_agent_permission(
    just_binary: &str,
    recipe: &str,
    working_dir: Option<&str>,
    justfile: Option<&str>,
) -> String {
    let output = run_just(
        just_binary,
        &["--dump", "--dump-format", "json"],
        working_dir,
        justfile,
    )
    .await;

    let output = match output {
        Ok(o) if o.success => o,
        _ => return "per-request".to_string(),
    };

    let dump: Value = match serde_json::from_str(&output.stdout) {
        Ok(v) => v,
        Err(_) => return "per-request".to_string(),
    };

    let Some(recipe_obj) = dump.get("recipes").and_then(|r| r.get(recipe)) else {
        return "per-request".to_string();
    };

    let Some(attributes) = recipe_obj.get("attributes").and_then(|a| a.as_array()) else {
        return "per-request".to_string();
    };

    for attr in attributes {
        if let Some(value) = attr.get("agents").and_then(|v| v.as_str()) {
            return value.to_string();
        }
    }

    "per-request".to_string()
}

pub async fn run_just(
    just_binary: &str,
    args: &[&str],
    working_dir: Option<&str>,
    justfile: Option<&str>,
) -> Result<JustOutput, String> {
    let mut cmd = Command::new(just_binary);

    if let Some(dir) = working_dir {
        cmd.arg("--working-directory").arg(dir);
    }

    if let Some(jf) = justfile {
        cmd.arg("--justfile").arg(jf);
    }

    cmd.args(args);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to execute {just_binary}: {e}"))?;

    Ok(JustOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        success: output.status.success(),
    })
}
