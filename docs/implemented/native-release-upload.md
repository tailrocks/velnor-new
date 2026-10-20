# Native release asset upload

The native upload transport uses the fixed `uploads.github.com` host. GitHub’s
[REST asset-upload contract](https://docs.github.com/en/rest/releases/assets?apiVersion=2026-03-10#upload-a-release-asset)
requires the raw binary body, its content type and content length. Asset names
are validated before mutation because GitHub renames leading or trailing periods.

The qualified [GH 2.102.0 API transport](https://github.com/cli/cli/blob/v2.102.0/pkg/cmd/api/http.go)
accepts an absolute URL without replacing its host. Its
[HTTP client](https://github.com/cli/cli/blob/v2.102.0/api/http_client.go) normalizes
the authentication host; pinned [go-gh 2.16.1](https://github.com/cli/go-gh/blob/v2.16.1/pkg/auth/auth.go)
maps GitHub subdomains to `github.com` before selecting `GH_TOKEN`. The
[API input owner](https://github.com/cli/cli/blob/v2.102.0/pkg/cmd/api/api.go)
opens the input file and supplies its actual content length.

Source inspection and the isolated argument-capture regression establish this
compatibility. They do not claim a live credentialed upload or hosted capacity.
