# symlink-escape fixture
Intent: symlink hazards for plan/preview guards.
Contains: escape/ -> outside root (/tmp/...),
loop -> itself (cycle).
Expected detector outcome: index/scan refuses
symlinks escaping root or looping; plan leaves
repo byte-identical; preview with such output
refused.
