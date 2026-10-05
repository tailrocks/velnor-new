# empty-suite fixture
Intent: a library with no unit-test or doctest target, using the Nextest profile.
The manifest sets `test = false` and `doctest = false`, so Cargo metadata
proves there is no applicable test target. The fixture explicitly selects
Nextest; Velnor records `valid_no_test_targets` and emits no test command. A
library test target that exists but discovers zero tests remains a failing
selected suite.
