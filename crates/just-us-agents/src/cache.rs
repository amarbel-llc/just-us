use std::io;
use std::path::{Path, PathBuf};

pub struct ResultCache {
    base_dir: PathBuf,
}

impl ResultCache {
    pub fn new() -> io::Result<Self> {
        let base_dir = match std::env::var("JUST_US_AGENTS_CACHE_DIR") {
            Ok(dir) => PathBuf::from(dir),
            Err(_) => dirs::cache_dir()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no cache directory"))?
                .join("just-us-agents"),
        };

        Ok(ResultCache { base_dir })
    }

    pub fn cache_path(
        &self,
        working_dir: Option<&str>,
        justfile: Option<&str>,
        git_commit: &str,
        command_args: &[&str],
    ) -> PathBuf {
        let path_digest = {
            let mut hasher = blake3::Hasher::new();
            if let Some(wd) = working_dir {
                hasher.update(wd.as_bytes());
            }
            if let Some(jf) = justfile {
                hasher.update(jf.as_bytes());
            }
            hasher.finalize().to_hex()[..16].to_string()
        };

        let command_digest = {
            let mut hasher = blake3::Hasher::new();
            for arg in command_args {
                hasher.update(arg.as_bytes());
                hasher.update(b"\0");
            }
            hasher.finalize().to_hex()[..16].to_string()
        };

        let filename = format!(
            "{}-{}.just-us-agents-command-result",
            git_commit, command_digest
        );

        self.base_dir.join(&path_digest).join(filename)
    }

    pub fn store(&self, path: &Path, content: &str) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)
    }

    pub fn read(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    pub fn read_by_components(&self, path_digest: &str, filename: &str) -> io::Result<String> {
        let path = self.base_dir.join(path_digest).join(filename);
        self.read(&path)
    }

    pub fn cleanup(&self) -> io::Result<()> {
        if self.base_dir.exists() {
            std::fs::remove_dir_all(&self.base_dir)?;
        }
        Ok(())
    }
}

pub async fn git_commit_short(working_dir: Option<&str>) -> String {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["rev-parse", "--short", "HEAD"]);

    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }

    match cmd.output().await {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "no-git".to_string(),
    }
}

/// Build a resource URI from cache path components.
pub fn cache_uri(path_digest: &str, filename: &str) -> String {
    format!("just-us://results/{}/{}", path_digest, filename)
}

/// Extract (path_digest, filename) from a `just-us://results/...` URI.
pub fn parse_cache_uri(uri: &str) -> Option<(String, String)> {
    let rest = uri.strip_prefix("just-us://results/")?;
    let (path_digest, filename) = rest.split_once('/')?;
    Some((path_digest.to_string(), filename.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_path_deterministic() {
        let cache = ResultCache {
            base_dir: PathBuf::from("/tmp/test-cache"),
        };

        let p1 = cache.cache_path(Some("/home/user/project"), Some("justfile"), "abc1234", &["build"]);
        let p2 = cache.cache_path(Some("/home/user/project"), Some("justfile"), "abc1234", &["build"]);
        assert_eq!(p1, p2);
    }

    #[test]
    fn cache_path_differs_by_command() {
        let cache = ResultCache {
            base_dir: PathBuf::from("/tmp/test-cache"),
        };

        let p1 = cache.cache_path(Some("/project"), None, "abc1234", &["build"]);
        let p2 = cache.cache_path(Some("/project"), None, "abc1234", &["test"]);
        assert_ne!(p1, p2);
    }

    #[test]
    fn cache_path_differs_by_path() {
        let cache = ResultCache {
            base_dir: PathBuf::from("/tmp/test-cache"),
        };

        let p1 = cache.cache_path(Some("/project-a"), None, "abc1234", &["build"]);
        let p2 = cache.cache_path(Some("/project-b"), None, "abc1234", &["build"]);
        assert_ne!(p1, p2);
    }

    #[test]
    fn cache_path_structure() {
        let cache = ResultCache {
            base_dir: PathBuf::from("/tmp/test-cache"),
        };

        let path = cache.cache_path(Some("/project"), None, "abc1234", &["build"]);
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("abc1234-"));
        assert!(filename.ends_with(".just-us-agents-command-result"));
        assert_eq!(path.parent().unwrap().parent().unwrap(), Path::new("/tmp/test-cache"));
    }

    #[test]
    fn uri_roundtrip() {
        let uri = cache_uri("abcdef0123456789", "abc1234-deadbeef01234567.just-us-agents-command-result");
        let (path_digest, filename) = parse_cache_uri(&uri).unwrap();
        assert_eq!(path_digest, "abcdef0123456789");
        assert_eq!(filename, "abc1234-deadbeef01234567.just-us-agents-command-result");
    }

    #[test]
    fn parse_invalid_uri() {
        assert!(parse_cache_uri("http://example.com").is_none());
        assert!(parse_cache_uri("just-us://results/").is_none());
        assert!(parse_cache_uri("just-us://results/abc").is_none());
    }
}
