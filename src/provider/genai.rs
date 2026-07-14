use async_trait::async_trait;
use genai::adapter::AdapterKind;
use genai::resolver::{AuthData, Endpoint, ServiceTargetResolver};
use genai::{Client, ModelIden, ServiceTarget};
use tokio_util::sync::CancellationToken;

use crate::provider::{ModelRequest, Provider, ProviderError, ProviderStream};

#[derive(Debug, Clone)]
pub struct GenaiProvider {
    client: Client,
    model: String,
}

impl GenaiProvider {
    pub fn from_env(
        base_url: &str,
        model: &str,
        api_key_env: &str,
    ) -> Result<Self, ProviderError> {
        let api_key = std::env::var(api_key_env).map_err(|_| ProviderError::Upstream {
            code: "missing_api_key".to_owned(),
            message: format!("env var {api_key_env} not set"),
            retryable: false,
        })?;
        Ok(Self::with_api_key(base_url, model, api_key))
    }

    #[must_use]
    pub fn with_api_key(base_url: &str, model: &str, api_key: String) -> Self {
        let endpoint = Endpoint::from_owned(base_url.to_owned());
        let auth = AuthData::from_single(api_key);
        let resolver = ServiceTargetResolver::from_resolver_fn(
            move |target: ServiceTarget| -> Result<ServiceTarget, genai::resolver::Error> {
                let ServiceTarget { model, .. } = target;
                let model = ModelIden::new(AdapterKind::OpenAI, model.model_name);
                Ok(ServiceTarget {
                    endpoint: endpoint.clone(),
                    auth: auth.clone(),
                    model,
                })
            },
        );
        let client = Client::builder()
            .with_service_target_resolver(resolver)
            .build();

        Self {
            client,
            model: model.to_owned(),
        }
    }
}

#[async_trait]
impl Provider for GenaiProvider {
    async fn stream(
        &self,
        _req: ModelRequest,
        _cancel: CancellationToken,
    ) -> Result<ProviderStream, ProviderError> {
        Err(ProviderError::Upstream {
            code: "genai_not_implemented".to_owned(),
            message: "genai streaming is not implemented".to_owned(),
            retryable: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::GenaiProvider;

    #[test]
    fn from_env_missing_key_returns_err() {
        const ENV_NAME: &str = "CROW_TEST_MISSING_GENAI_API_KEY_2_2";
        // SAFETY: this test uses a task-specific variable that no other test reads.
        unsafe { std::env::remove_var(ENV_NAME) };

        let result = GenaiProvider::from_env("https://example.invalid/v1", "test-model", ENV_NAME);

        assert!(result.is_err());
    }

    #[test]
    fn with_api_key_constructs() {
        let provider = GenaiProvider::with_api_key(
            "https://example.invalid/v1",
            "test-model",
            "test-key".to_owned(),
        );

        let _ = provider;
    }
}
