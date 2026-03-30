use crate::cache::{ResultCache, parse_cache_uri};
use async_trait::async_trait;
use mcp_server::Context;
use mcp_server::resources::{Resource, ResourceContent, ResourceError};
use std::sync::Arc;

pub struct ResultResource {
  pub cache: Arc<ResultCache>,
}

#[async_trait]
impl Resource for ResultResource {
  fn uri_template(&self) -> &str {
    "just-us://results/{path_digest}/{filename}"
  }

  fn name(&self) -> &str {
    "command-results"
  }

  fn description(&self) -> &str {
    "Full output from recipe execution"
  }

  fn mime_type(&self) -> &str {
    "text/plain"
  }

  async fn read(&self, uri: &str, _ctx: &Context) -> Result<ResourceContent, ResourceError> {
    let (path_digest, filename) = parse_cache_uri(uri)
      .ok_or_else(|| ResourceError::InvalidUri(format!("invalid result URI: {uri}")))?;

    let content = self
      .cache
      .read_by_components(&path_digest, &filename)
      .map_err(|e| ResourceError::ReadFailed(format!("failed to read cache: {e}")))?;

    Ok(ResourceContent {
      uri: uri.to_string(),
      mime_type: "text/plain".to_string(),
      text: content,
    })
  }
}
