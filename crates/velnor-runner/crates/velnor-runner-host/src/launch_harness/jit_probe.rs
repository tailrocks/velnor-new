use super::*;

pub(crate) struct JitProbe {
    inner: Script,
    pub(crate) names: Vec<String>,
    conflict: bool,
}

impl JitProbe {
    pub(crate) fn conflict() -> Self {
        Self {
            inner: Script {
                calls: Vec::new(),
                mode: Mode::Ok,
            },
            names: Vec::new(),
            conflict: true,
        }
    }

    pub(crate) fn calls(&self) -> &[&'static str] {
        &self.inner.calls
    }
}

impl Transport for JitProbe {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        if request.path.contains("generatejitconfig") {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&request.body)
                && let Some(name) = value.get("name").and_then(|item| item.as_str())
            {
                self.names.push(name.to_owned());
            }
            if self.conflict {
                self.inner.calls.push("jit");
                return Err(TransportFail::Http(409));
            }
        }
        self.inner.exchange(request)
    }
}

impl Lane for JitProbe {
    fn on_admin(&mut self) -> Result<(), EnsureError> {
        self.inner.on_admin()
    }

    fn on_queue(&mut self) -> Result<(), EnsureError> {
        self.inner.on_queue()
    }

    fn use_github_api(&mut self) -> Result<(), EnsureError> {
        self.inner.use_github_api()
    }
}
