use tokio::process::Command;

pub struct JustOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
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
