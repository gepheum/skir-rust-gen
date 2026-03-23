use super::method::Method;
use super::serializer::{JsonFlavor, UnrecognizedValues};

// =============================================================================
// RpcError
// =============================================================================

/// Error returned by [`ServiceClient::invoke_remote`] when the server responds
/// with a non-2xx status code or when a network-level failure occurs.
#[derive(Debug, Clone, thiserror::Error)]
#[error("rpc error {status_code}: {message}")]
pub struct RpcError {
    /// The HTTP status code returned by the server, or `0` for network-level
    /// failures (e.g. DNS error, connection refused, timeout).
    pub status_code: u16,
    /// A human-readable description of the error.
    pub message: String,
}

// =============================================================================
// ServiceClient
// =============================================================================

/// Sends RPCs to a Skir service.
///
/// # Example
///
/// ```no_run
/// # use e2e_test::skir_client::service_client::ServiceClient;
/// let client = ServiceClient::new("http://localhost:8787/myapi").unwrap();
/// // let resp = client.invoke_remote(my_method, &request, &[]).unwrap();
/// ```
pub struct ServiceClient {
    service_url: String,
    default_headers: Vec<(String, String)>,
    http_client: ureq::Agent,
}

impl ServiceClient {
    /// Creates a `ServiceClient` pointing at `service_url`.
    ///
    /// # Errors
    ///
    /// Returns an error if `service_url` contains a query string.
    pub fn new(service_url: impl Into<String>) -> Result<Self, String> {
        let url = service_url.into();
        if url.contains('?') {
            return Err("service URL must not contain a query string".to_owned());
        }
        Ok(Self {
            service_url: url,
            default_headers: Vec::new(),
            http_client: ureq::AgentBuilder::new().build(),
        })
    }

    /// Adds a default HTTP header sent with every invocation.
    ///
    /// Can be chained: `client.with_default_header("Authorization", "Bearer …")`.
    pub fn with_default_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.default_headers.push((key.into(), value.into()));
        self
    }

    /// Invokes `method` on the remote service with the given `request`.
    ///
    /// `extra_headers` is a slice of `(name, value)` pairs added (or
    /// overriding) the default headers for this specific call only.
    ///
    /// The request is serialized as dense JSON.  The response is deserialized
    /// keeping any unrecognized values (the server may have a newer schema).
    ///
    /// # Errors
    ///
    /// Returns [`RpcError`] if the server responds with a non-2xx status code
    /// or if a network-level failure occurs.
    pub fn invoke_remote<Req, Resp>(
        &self,
        method: &Method<Req, Resp>,
        request: &Req,
        extra_headers: &[(&str, &str)],
    ) -> Result<Resp, RpcError>
    where
        Req: 'static,
        Resp: 'static,
    {
        // Serialize the request as dense JSON.
        let request_json = method
            .request_serializer
            .to_json(request, JsonFlavor::Dense);

        // Wire body: "MethodName:number::requestJson"
        // The empty third field means the server may reply in dense JSON.
        let body = format!("{}:{}::{}", method.name, method.number, request_json);

        let mut req = self
            .http_client
            .post(&self.service_url)
            .set("Content-Type", "text/plain; charset=utf-8");
        for (k, v) in &self.default_headers {
            req = req.set(k, v);
        }
        for &(k, v) in extra_headers {
            req = req.set(k, v);
        }

        let resp = req.send_string(&body).map_err(|e| match e {
            ureq::Error::Status(status_code, resp) => {
                let message = if resp
                    .header("content-type")
                    .map(|ct| ct.contains("text/plain"))
                    .unwrap_or(false)
                {
                    resp.into_string().unwrap_or_default()
                } else {
                    String::new()
                };
                RpcError {
                    status_code,
                    message,
                }
            }
            ureq::Error::Transport(t) => RpcError {
                status_code: 0,
                message: t.to_string(),
            },
        })?;

        let json_code = resp.into_string().map_err(|e| RpcError {
            status_code: 0,
            message: format!("failed to read response body: {e}"),
        })?;

        method
            .response_serializer
            .from_json(&json_code, UnrecognizedValues::Keep)
            .map_err(|e| RpcError {
                status_code: 0,
                message: format!("failed to decode response: {e}"),
            })
    }
}
