# FICTIONAL lock: hashes bogus, shape only. Deliberately non-canonical
# spacing: proves fmt never visits the lockfile.
provider "example.com/a/b" {
version="1.0.0"
constraints="~> 1.0"
hashes=["h1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="]
}
