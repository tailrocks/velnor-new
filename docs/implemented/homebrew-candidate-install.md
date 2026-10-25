# Native Homebrew candidate installation

The native coordinator installs an authenticated canonical formula through a
private physical Tap in the fresh, qualified Homebrew prefix. It does not load
an arbitrary formula directly from the caller's staging directory.

The qualified Homebrew source is
[`7a056ffd71e505f725b092d162fb09164a62b510`](https://github.com/Homebrew/brew/tree/7a056ffd71e505f725b092d162fb09164a62b510).
Its [path admission](https://github.com/Homebrew/brew/blob/7a056ffd71e505f725b092d162fb09164a62b510/Library/Homebrew/utils/path.rb)
requires formula paths under a Tap or the Cellar. The normal
[Brew launcher](https://github.com/Homebrew/brew/blob/7a056ffd71e505f725b092d162fb09164a62b510/Library/Homebrew/brew.sh)
removes the internal path-allowance variable before command dispatch. Developer,
test, and global trust overrides are not part of this installation contract.

The Tap identity is `velnor/native-install`. Its only discoverable formula is
`Library/Taps/velnor/homebrew-native-install/Formula/<formula>.rb`. The coordinator
uses the supported `brew trust --formula velnor/native-install/<formula>` command
with a private HOME. It then installs or upgrades the exact absolute loader path.
No remote Tap is fetched and no Git repository is needed for this private Tap.

The canonical A and D staging files remain distinct and immutable. Installation
holds the canonical file and its Tap loader copy through the archived recipe
test and installed-file checks. Before an A-to-D upgrade, the coordinator verifies
the old loader, canonical A, archived A recipe, receipt, payload, and keg custody.
Only the stable Tap loader is atomically replaced with D. The new loader and
canonical D remain held while D and the preserved A archive are tested.

Receipts bind the exact stable loader pathname and exact Tap identity. Each
installed version retains its own unchanged receipt, archived recipe, payload,
and keg identity. A receipt's source pathname can therefore point to the current
Tap formula after an upgrade; the archived recipe and canonical staging file
independently bind the bytes that installed that particular version.

Unexpected Tap content, foreign state, symlinks, hardlinks, stale loader bytes,
and incorrect receipt identities are rejected. The existing absolute deadline,
credential-scrubbed command environment, and process-group drain requirements
continue to apply. Real hosted A-to-D installation and archived tests are required
before SDK publication; unit tests do not replace that qualification.

The qualified cleaner normalizes executables in `bin` to `0555`. Native
installed-host inventory and receipt evidence require that exact final mode.
Archive admission still checks the published archive policy. Auxiliary files in
`libexec` retain the formula's explicit `0644` or `0755` mode; the coordinator
neither accepts alternate host modes nor disables Homebrew cleaning.
