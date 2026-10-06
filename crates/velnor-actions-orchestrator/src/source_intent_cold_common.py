"""SourceIntent failures are fatal; they never request cache repair or reinstall."""


class ColdSourceIntent(ValueError):
    """The fixed cold installation or its immutable proof is unavailable/changed."""
